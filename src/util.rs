use std::{io::stdin, sync::mpsc, thread, time::Duration};
use termion::{event::Key, input::TermRead};

pub enum Event<I> {
    Input(I),
    Tick,
}

pub struct Events {
    rx: mpsc::Receiver<Event<Key>>,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub tick_rate: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            tick_rate: Duration::from_millis(1000),
        }
    }
}

impl Events {
    pub fn new() -> Events {
        Events::with_config(Config::default())
    }

    pub fn with_config(config: Config) -> Events {
        let (tx, rx) = mpsc::channel();
        let _input_handle = {
            let tx = tx.clone();
            thread::spawn(move || {
                let stdin_result = stdin();
                for evt in stdin_result.keys() {
                    if let Ok(key) = evt {
                        if tx.send(Event::Input(key)).is_err() {
                            return;
                        }
                    }
                }

                tx.send(Event::Tick).unwrap();
                thread::sleep(config.tick_rate);
            })
        };

        Events { rx }
    }

    pub fn next(&self) -> Result<Event<Key>, mpsc::RecvError> {
        self.rx.recv()
    }
}
