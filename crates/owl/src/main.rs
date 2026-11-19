//! owl: one synthetic LogosChat client.
//!
//! An owl gets an identity and the settings from roost with the flock key,
//! builds its chat client on an embedded logos-delivery node, creates a group,
//! and then adds other owls to groups and posts to them until it is stopped,
//! reporting what it sends, receives and sees of its groups.

mod anomaly;
mod group;
mod owl;
mod recorder;
mod roost;

use clap::Parser;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Parser, Debug)]
#[command(version)]
pub struct Args {
    #[arg(long, env = "ROOST_URL", default_value = "http://localhost:30117")]
    roost: String,
    /// The flock key, the same for every owl. roost assigns this owl process
    /// an identity.
    #[arg(long, env = "FLOCK_KEY", hide_env_values = true)]
    key: String,
    /// Names this owl across restarts, so it keeps its identity. Defaults to
    /// the ID in `./instance`, made on first start; in Docker the working
    /// directory is a volume, so it survives the container being recreated.
    #[arg(long, env = "OWL_INSTANCE")]
    instance: Option<String>,
    /// Who runs this owl: a name or handle. roost shows it next to the owl,
    /// so whoever watches the flock knows whom to ask about it.
    #[arg(long, env = "OWL_OPERATOR")]
    operator: String,
    /// Run as an ephemeral owl: roost gives it a name such as `owlet-01`, and
    /// can give that name to another ephemeral owl two minutes after this one
    /// stops. Named owls keep their names.
    #[arg(long, env = "OWL_EPHEMERAL", value_parser = clap::builder::BoolishValueParser::new())]
    ephemeral: bool,
    /// The embedded logos-delivery node's log level: TRACE, DEBUG, INFO,
    /// NOTICE, WARN, ERROR or FATAL. It logs to stdout.
    #[arg(long, env = "DELIVERY_LOG_LEVEL", default_value = "ERROR")]
    delivery_log_level: String,
}

fn main() -> anyhow::Result<()> {
    // The filter only decides what is printed: libchat's warnings and errors
    // go to roost whatever it says.
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "owl=info".into());
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter))
        .with(anomaly::LogLayer)
        .init();
    owl::run(Args::parse())
}
