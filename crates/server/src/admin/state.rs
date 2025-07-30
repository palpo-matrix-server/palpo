use std::sync::OnceLock;
use std::{
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
    sync::{Arc, RwLock as StdRwLock, Weak},
    time::Instant,
};

use tokio::sync::{RwLock, mpsc};
use tokio::{runtime, sync::broadcast};

use crate::admin::{CommandInput, Completer, Console, Processor};

pub static STATE: OnceLock<State> = OnceLock::new();
pub struct State {
    pub signal: broadcast::Sender<&'static str>,
    pub channel: StdRwLock<Option<mpsc::Sender<CommandInput>>>,
    pub handle: RwLock<Option<Processor>>,
    pub complete: StdRwLock<Option<Completer>>,
    pub console: Arc<Console>,
}

pub fn get() -> &'static State {
    STATE.get_or_init(|| State {
        signal: broadcast::channel::<&'static str>(1).0,
        channel: StdRwLock::new(None),
        handle: RwLock::new(None),
        complete: StdRwLock::new(None),
        console: Console::new(),
    })
}
