use alloc::sync::Arc;
use crate::objects::KernelObject;
use crate::objects::process_object::ProcessObject;
use crate::objects::thread_object::ThreadObject;
use crate::task::process::Process;
use crate::task::thread::Thread;

/// Register a process in the object namespace.
pub fn register_process(proc: Arc<Process>) -> Arc<ProcessObject> {
    let obj = ProcessObject::new(proc);
    let name = obj.query_name().unwrap_or_else(|| alloc::format!("Process/{}", obj.inner.lock().id));
    crate::objects::namespace::register_object(&name, obj.clone());
    obj
}

/// Register a thread in the object namespace.
pub fn register_thread(thread: Thread) -> Arc<ThreadObject> {
    let obj = ThreadObject::new(thread);
    let name = alloc::format!("Thread/{:?}", obj.inner.lock()._id);
    crate::objects::namespace::register_object(&name, obj.clone());
    obj
}
