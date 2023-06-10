use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use crossbeam_channel::Sender;
use duct::Expression;

use super::ExternalEvent;

pub fn run_cmd(
    cmd: Expression,
    sender: Sender<ExternalEvent>,
    refresh_time: Duration,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let handle = cmd.start()?;
    thread::spawn(move || loop {
        if stop.load(Ordering::Acquire) {
            if let Err(err) = handle.kill() {
                sender
                    .send(ExternalEvent::Error(err.into()))
                    .expect("sender should not have deallocated");
            }
            return;
        }
        match handle.try_wait() {
            Ok(Some(out)) => {
                sender
                    .send(ExternalEvent::CommandOutput(
                        String::from_utf8_lossy(if out.stdout.is_empty() {
                            &out.stderr
                        } else {
                            &out.stdout
                        })
                        .to_string(),
                    ))
                    .expect("sender should not have deallocated");
                return;
            }
            Ok(None) => {}
            Err(err) => {
                sender
                    .send(ExternalEvent::Error(err.into()))
                    .expect("sender should not have deallocated");
                return;
            }
        };
        thread::sleep(refresh_time);
    });

    Ok(())
}
