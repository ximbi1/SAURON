use anyhow::{Context, Result};
use clap::Parser;
use sauron::{
    app::{Runtime, event::Payload},
    config::Config,
    kube::{ConnectOptions, watch::Query},
};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(name=sauron::brand::BINARY,version=sauron::brand::VERSION,about="One eye over the entire cluster. Inspection-only development release.")]
struct Cli {
    /// Resource plural/kind/shortname, or info
    resource: Option<String>,
    #[arg(short = 'n', long, conflicts_with = "all_namespaces")]
    namespace: Option<String>,
    #[arg(short = 'A', long)]
    all_namespaces: bool,
    #[arg(long)]
    context: Option<String>,
    #[arg(long)]
    kubeconfig: Option<PathBuf>,
    #[arg(long)]
    config: Option<PathBuf>,
    /// All operations are read-only in this release
    #[arg(long)]
    readonly: bool,
    /// Run discovery and report capabilities, without a TTY
    #[arg(long, conflicts_with = "snapshot")]
    check: bool,
    /// Read one synchronized resource list and exit
    #[arg(long)]
    snapshot: bool,
    /// Only valid with info; never loads kubeconfig or connects
    #[arg(long)]
    offline: bool,
    #[arg(short = 'l', long)]
    selector: Option<String>,
    #[arg(long)]
    field_selector: Option<String>,
    #[arg(long)]
    filter: Option<String>,
    #[arg(long,default_value="text",value_parser=["text","json","yaml"])]
    output: String,
    #[arg(long,default_value_t=30,value_parser=clap::value_parser!(u64).range(1..=300))]
    timeout: u64,
}

#[tokio::main(worker_threads = 2)]
async fn main() {
    if let Err(error) = start().await {
        eprintln!(
            "{}: {}",
            sauron::brand::NAME,
            sauron::safety::text(&error.to_string())
        );
        std::process::exit(1);
    }
}
async fn start() -> Result<()> {
    let cli = Cli::parse();
    let is_info = cli.resource.as_deref() == Some("info");
    anyhow::ensure!(
        !cli.offline || is_info,
        "--offline is supported only by info"
    );
    // Tracing is opt-in on headless stderr; interactive stderr is the terminal screen.
    // External library logs are excluded to avoid credential-bearing debug fields.
    if (cli.check || cli.snapshot || is_info) && std::env::var_os("SAURON_LOG").is_some() {
        tracing_subscriber::fmt()
            .with_env_filter("off,sauron=info")
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .try_init()
            .ok();
    }
    let config = Config::load(cli.config.as_deref())?;
    let settings = config.resolve("", "")?;
    if is_info && cli.offline {
        println!(
            "{} {}\nOffline: no kubeconfig loaded, no Kubernetes requests\nConfig directory: {}\nRead-only: enforced\nTheme: {}\nMax objects: {}\nCache JSON budget: {} bytes\nNo telemetry. Plugins and mutations unavailable.",
            sauron::brand::NAME,
            sauron::brand::VERSION,
            sauron::config::directory().display(),
            settings.theme,
            settings.max_objects,
            settings.max_bytes
        );
        return Ok(());
    }
    let query = Query {
        resource: if is_info {
            settings.resource.clone()
        } else {
            cli.resource.unwrap_or_else(|| settings.resource.clone())
        },
        namespace: if cli.all_namespaces {
            None
        } else {
            Some(cli.namespace.unwrap_or_default())
        },
        labels: cli.selector,
        fields: cli.field_selector,
    };
    let options = ConnectOptions {
        kubeconfig: cli.kubeconfig,
        context: cli.context,
        force_readonly: cli.readonly,
    };
    let (mut runtime, mut rx) = Runtime::new(options, config, cli.config, query)?;
    if let Some(filter) = cli.filter {
        runtime.state.filter = sauron::filters::Expr::parse(&filter)?;
        runtime.state.filter_text = filter;
    }
    if !(cli.check || cli.snapshot || is_info) {
        return sauron::app::run(runtime, rx).await;
    }
    runtime.connect(None);
    let result=tokio::time::timeout(Duration::from_secs(cli.timeout),async {
        loop {
            let event=rx.recv().await.context("Background channel closed")?;
            let relevant=event.epoch==runtime.state.epoch;
            let ready=relevant && matches!(event.payload,Payload::Ready);
            let connected=relevant && matches!(event.payload,Payload::Connected(_));
            runtime.reduce(event);
            if let Some(error)=&runtime.state.error{anyhow::bail!("{error}");}
            if connected && (cli.check||is_info){print!("{}",runtime.info());break;}
            if ready {
                runtime.state.rebuild();
                if runtime.state.filter_unknown > 0 {
                    eprintln!("FILTER UNKNOWN: {} rows excluded because required values are unavailable or invalid; not a FALSE result", runtime.state.filter_unknown);
                }
                anyhow::ensure!(!runtime.state.store.incomplete,"Snapshot exceeds cache limits; narrow the namespace/selectors");
                match cli.output.as_str(){
                    "json"|"yaml"=>{
                        let document=serde_json::json!({"context":runtime.state.context,"namespace":runtime.state.query.namespace,"resource":runtime.state.query.resource,"filter":runtime.state.filter_text,"unknownExcluded":runtime.state.filter_unknown,"labelSelector":runtime.state.query.labels,"fieldSelector":runtime.state.query.fields,"collectedAt":chrono::Utc::now().to_rfc3339(),"items":runtime.state.rows.iter().map(|o|&o.value).collect::<Vec<_>>()});
                        println!("{}",if cli.output=="json"{serde_json::to_string_pretty(&document)?}else{serde_yaml_ng::to_string(&document)?});
                    },
                    _=>{let columns=runtime.state.columns();println!("{}",columns.join("\t"));for object in &runtime.state.rows{println!("{}",columns.iter().map(|c|sauron::safety::text(&if c=="AGE"{sauron::resources::age_text(object.age(chrono::Utc::now()))}else{object.field(c,chrono::Utc::now()).unwrap_or_else(||"-".into())}).replace(['\n','\t']," ")).collect::<Vec<_>>().join("\t"));}}
                }
                if let Some(c)=&runtime.connection {for warning in &c.catalog.warnings{eprintln!("PARTIAL DISCOVERY: {warning}");}}
                break;
            }
        }
        Ok::<_,anyhow::Error>(())
    }).await.context("Headless operation timed out; check connectivity or increase --timeout");
    runtime.shutdown().await;
    result??;
    Ok(())
}
