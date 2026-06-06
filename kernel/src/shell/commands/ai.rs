#![cfg(feature = "ai_rule")]
use crate::println;

pub fn vahiai(args: &[&str]) {
    if !args.is_empty() {
        let intent_name = args[0];
        let intent_args = &args[1..];
        let engine = crate::vahiai::ENGINE.lock();
        match engine.execute(intent_name, intent_args) {
            crate::vahiai::IntentResult::Success(msg) => println!("VahiAI: Success: {}", msg),
            crate::vahiai::IntentResult::Error(err) => println!("VahiAI: Error: {}", err),
            crate::vahiai::IntentResult::ExecuteSyscall(n, syscall_args) => {
                println!("VahiAI: Triggering Syscall {}...", n);
                {
                     crate::syscalls::syscall_handler(n, syscall_args[0], syscall_args[1], syscall_args[2], syscall_args[3], syscall_args[4], core::ptr::null_mut());
                }
            }
        }
    } else {
        println!("Usage: vahiai <intent> [args...]");
    }
}
