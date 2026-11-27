//! Ctrl-C while `backup` has Rakazo stopped must not leave it stopped (spec §15). While an
//! [`Interrupt`] is held the program survives SIGINT and SIGTERM, and only notes them. The
//! Docker commands it runs get the default action back when they start, so the one running
//! dies, the step fails, and the run starts Rakazo again before it stops.

use std::io;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};

const SIGNALS: [libc::c_int; 2] = [libc::SIGINT, libc::SIGTERM];

static CAUGHT: AtomicBool = AtomicBool::new(false);

extern "C" fn note(_signal: libc::c_int) {
    CAUGHT.store(true, Ordering::SeqCst);
}

/// Puts back the actions it replaced when dropped, so Ctrl-C ends the program again.
pub struct Interrupt {
    previous: Vec<(libc::c_int, libc::sigaction)>,
}

impl Interrupt {
    pub fn catch() -> io::Result<Self> {
        CAUGHT.store(false, Ordering::SeqCst);
        let mut interrupt = Self {
            previous: Vec::new(),
        };
        for signal in SIGNALS {
            // SAFETY: `note` only stores to an atomic, which is async-signal-safe, and both
            // structs are fully initialised before `sigaction` reads them.
            let previous = unsafe {
                let mut action: libc::sigaction = MaybeUninit::zeroed().assume_init();
                action.sa_sigaction = note as extern "C" fn(libc::c_int) as libc::sighandler_t;
                action.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut action.sa_mask);
                let mut previous: libc::sigaction = MaybeUninit::zeroed().assume_init();
                if libc::sigaction(signal, &action, &mut previous) != 0 {
                    return Err(io::Error::last_os_error());
                }
                previous
            };
            interrupt.previous.push((signal, previous));
        }
        Ok(interrupt)
    }

    pub fn caught(&self) -> bool {
        CAUGHT.load(Ordering::SeqCst)
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        for (signal, previous) in self.previous.drain(..) {
            // SAFETY: `previous` is the action `sigaction` handed back for this signal.
            unsafe {
                libc::sigaction(signal, &previous, std::ptr::null_mut());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_ctrl_c_survives_it_and_puts_the_old_action_back() {
        let current = || unsafe {
            let mut action: libc::sigaction = MaybeUninit::zeroed().assume_init();
            libc::sigaction(libc::SIGINT, std::ptr::null(), &mut action);
            action.sa_sigaction
        };
        let before = current();
        let interrupt = Interrupt::catch().unwrap();
        assert!(!interrupt.caught());
        unsafe { libc::raise(libc::SIGINT) };
        assert!(interrupt.caught());
        drop(interrupt);
        assert_eq!(current(), before);
    }
}
