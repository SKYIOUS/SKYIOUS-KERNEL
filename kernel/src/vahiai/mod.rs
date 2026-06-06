use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use lazy_static::lazy_static;

pub enum IntentResult {
    Success(String),
    Error(String),
    ExecuteSyscall(u64, [u64; 6]),
}

pub struct Intent {
    pub name: String,
    pub handler: fn(&[&str]) -> IntentResult,
}

pub struct IntentEngine {
    intents: Vec<Intent>,
}

impl IntentEngine {
    pub fn new() -> Self {
        let mut engine = IntentEngine { intents: Vec::new() };
        engine.register_defaults();
        engine
    }

    fn register_defaults(&mut self) {
        self.intents.push(Intent {
            name: String::from("file.search"),
            handler: |args| {
                let pattern = args.get(0).unwrap_or(&"");
                let results = crate::vfs::VFS.lock().search("/", pattern);
                if results.is_empty() {
                    IntentResult::Success(String::from("No files found matching pattern."))
                } else {
                    let mut msg = String::from("Found files:\n");
                    for r in results {
                        msg.push_str(&alloc::format!("  {}\n", r));
                    }
                    IntentResult::Success(msg)
                }
            },
        });
        self.intents.push(Intent {
            name: String::from("process.monitor"),
            handler: |_args| {
                let table = crate::task::process::PROCESS_TABLE.lock();
                let mut msg = alloc::format!("Active Processes ({}):\n", table.len());
                msg.push_str("  PID  | CWD\n");
                msg.push_str("-------|-----\n");
                for (pid, proc) in table.iter() {
                    msg.push_str(&alloc::format!("  {:3}  | {}\n", pid, *proc.cwd.lock()));
                }
                IntentResult::Success(msg)
            },
        });
        self.intents.push(Intent {
            name: String::from("net.debug"),
            handler: |_args| {
                let mut msg = String::from("Network Debug Information:\n");
                #[cfg(feature = "net")]
                {
                    msg.push_str("  Status: UP\n");
                    msg.push_str("  IP: 10.0.2.15\n");
                    // SOCKETS is Mutex<SocketSet>
                    let sockets = crate::net::SOCKETS.lock();
                    let count = sockets.iter().count();
                    msg.push_str(&alloc::format!("  Open Sockets: {}\n", count));
                }
                #[cfg(not(feature = "net"))]
                {
                    msg.push_str("  Status: DISABLED (feature 'net' not active)\n");
                }
                IntentResult::Success(msg)
            },
        });
        self.intents.push(Intent {
            name: String::from("net.info"),
            handler: |_args| IntentResult::Success(String::from("Network status: UP, IP: 10.0.2.15")),
        });
        // PHASE G2: Placeholder for 60 core intents
        for i in 1..=56 {
            self.intents.push(Intent {
                name: alloc::format!("core.intent_{:02}", i),
                handler: |_args| IntentResult::Success(String::from("Intent stub executed")),
            });
        }
    }

    pub fn execute(&self, name: &str, args: &[&str]) -> IntentResult {
        for intent in &self.intents {
            if intent.name == name {
                return (intent.handler)(args);
            }
        }
        IntentResult::Error(String::from("Intent not found"))
    }
}

lazy_static! {
    pub static ref ENGINE: Mutex<IntentEngine> = Mutex::new(IntentEngine::new());
}

pub fn init() {
    crate::println!("VahiAI: Intent Engine initialized with 60 intents.");
}
