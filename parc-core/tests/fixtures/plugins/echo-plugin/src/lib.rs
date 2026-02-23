#[link(wasm_import_module = "parc_host")]
extern "C" {
    fn parc_host_output(ptr: i32, len: i32);
    fn parc_host_log(level: i32, ptr: i32, len: i32);
}

static mut HEAP: [u8; 65536] = [0u8; 65536];
static mut HEAP_POS: usize = 0;

#[unsafe(no_mangle)]
pub extern "C" fn parc_alloc(size: i32) -> i32 {
    unsafe {
        let pos = HEAP_POS;
        let new_pos = pos + size as usize;
        if new_pos > HEAP.len() {
            return 0;
        }
        HEAP_POS = new_pos;
        HEAP.as_ptr().add(pos) as i32
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn parc_free(_ptr: i32, _size: i32) {
    // Simple bump allocator — no-op free
}

#[unsafe(no_mangle)]
pub extern "C" fn parc_plugin_init(_config_ptr: i32, _config_len: i32) -> i32 {
    let msg = b"echo plugin initialized";
    unsafe {
        parc_host_log(1, msg.as_ptr() as i32, msg.len() as i32);
    }
    0
}

/// Event handler: echoes back the event name and fragment JSON via output.
#[unsafe(no_mangle)]
pub extern "C" fn parc_on_event(
    event_ptr: i32,
    event_len: i32,
    fragment_ptr: i32,
    fragment_len: i32,
) -> i32 {
    // Build output: "event=<event> fragment=<fragment>"
    let prefix = b"event=";
    let mid = b" fragment=";

    let total = prefix.len() + event_len as usize + mid.len() + fragment_len as usize;
    let buf_ptr = parc_alloc(total as i32);
    if buf_ptr == 0 {
        return 0;
    }

    unsafe {
        let dst = buf_ptr as *mut u8;
        // Copy "event="
        core::ptr::copy_nonoverlapping(prefix.as_ptr(), dst, prefix.len());
        let mut offset = prefix.len();
        // Copy event
        core::ptr::copy_nonoverlapping(event_ptr as *const u8, dst.add(offset), event_len as usize);
        offset += event_len as usize;
        // Copy " fragment="
        core::ptr::copy_nonoverlapping(mid.as_ptr(), dst.add(offset), mid.len());
        offset += mid.len();
        // Copy fragment
        core::ptr::copy_nonoverlapping(fragment_ptr as *const u8, dst.add(offset), fragment_len as usize);

        parc_host_output(buf_ptr, total as i32);
    }

    1 // non-zero = produced output
}

/// Command handler: echoes the command and args back.
#[unsafe(no_mangle)]
pub extern "C" fn parc_command(
    cmd_ptr: i32,
    cmd_len: i32,
    args_ptr: i32,
    args_len: i32,
) -> i32 {
    let prefix = b"cmd=";
    let mid = b" args=";

    let total = prefix.len() + cmd_len as usize + mid.len() + args_len as usize;
    let buf_ptr = parc_alloc(total as i32);
    if buf_ptr == 0 {
        return 0;
    }

    unsafe {
        let dst = buf_ptr as *mut u8;
        core::ptr::copy_nonoverlapping(prefix.as_ptr(), dst, prefix.len());
        let mut offset = prefix.len();
        core::ptr::copy_nonoverlapping(cmd_ptr as *const u8, dst.add(offset), cmd_len as usize);
        offset += cmd_len as usize;
        core::ptr::copy_nonoverlapping(mid.as_ptr(), dst.add(offset), mid.len());
        offset += mid.len();
        core::ptr::copy_nonoverlapping(args_ptr as *const u8, dst.add(offset), args_len as usize);

        parc_host_output(buf_ptr, total as i32);
    }

    0
}
