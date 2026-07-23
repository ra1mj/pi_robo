use pi_model::{Cancellation, CancellationFuture};
use pi_store::CommandCancellation;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug)]
struct CancellationState {
    cancelled: AtomicBool,
    exit_code: AtomicU8,
    signal: watch::Sender<bool>,
}

#[derive(Clone, Debug)]
pub struct RootCancellation {
    state: Arc<CancellationState>,
}

impl Default for RootCancellation {
    fn default() -> Self {
        let (signal, _) = watch::channel(false);
        Self {
            state: Arc::new(CancellationState {
                cancelled: AtomicBool::new(false),
                exit_code: AtomicU8::new(0),
                signal,
            }),
        }
    }
}

impl RootCancellation {
    pub fn cancel(&self, exit_code: u8) {
        self.state
            .exit_code
            .compare_exchange(0, exit_code, Ordering::AcqRel, Ordering::Acquire)
            .ok();
        if !self.state.cancelled.swap(true, Ordering::AcqRel) {
            self.state.signal.send_replace(true);
        }
    }

    #[must_use]
    pub fn signal_exit_code(&self) -> Option<u8> {
        let value = self.state.exit_code.load(Ordering::Acquire);
        (value != 0).then_some(value)
    }

    fn cancellation_future(&self) -> CancellationFuture<'_> {
        let mut receiver = self.state.signal.subscribe();
        Box::pin(async move {
            if *receiver.borrow() {
                return;
            }
            while receiver.changed().await.is_ok() {
                if *receiver.borrow_and_update() {
                    return;
                }
            }
        })
    }
}

impl Cancellation for RootCancellation {
    fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    fn cancelled(&self) -> CancellationFuture<'_> {
        self.cancellation_future()
    }
}

impl CommandCancellation for RootCancellation {
    fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    fn cancelled(&self) -> pi_store::CancellationFuture<'_> {
        self.cancellation_future()
    }
}

#[cfg(unix)]
pub fn install_signal_handlers(cancellation: RootCancellation) -> JoinHandle<()> {
    use tokio::signal::unix::{SignalKind, signal};

    tokio::spawn(async move {
        let mut interrupt = match signal(SignalKind::interrupt()) {
            Ok(signal) => signal,
            Err(_) => return,
        };
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(_) => return,
        };
        let mut hangup = match signal(SignalKind::hangup()) {
            Ok(signal) => signal,
            Err(_) => return,
        };
        tokio::select! {
            _ = interrupt.recv() => cancellation.cancel(130),
            _ = terminate.recv() => cancellation.cancel(143),
            _ = hangup.recv() => cancellation.cancel(129),
        }
    })
}

#[cfg(not(unix))]
pub fn install_signal_handlers(cancellation: RootCancellation) -> JoinHandle<()> {
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            cancellation.cancel(130);
        }
    })
}
