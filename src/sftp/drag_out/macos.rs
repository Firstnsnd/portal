//! macOS glue for OS-level drag-out (app → Finder) via NSFilePromiseProvider.
//!
//! egui has no API for initiating a native drag session, and remote files do
//! not exist locally, so we register a drag-source delegate that promises the
//! dragged file names and downloads them on demand once Finder accepts the
//! drop and tells us the destination directory.

use std::sync::{Mutex, Once, OnceLock};

use cocoa::appkit::NSApp;
use cocoa::base::{id, nil};
use cocoa::foundation::{NSArray, NSAutoreleasePool, NSPoint, NSRect, NSSize, NSString};
use objc::{class, msg_send, sel, sel_impl};
use objc::declare::ClassDecl;
use objc::runtime::{Object, Sel};

use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use crate::sftp::types::SftpCommand;

use super::{PromiseRegistry, PromiseSpec};

/// NSDragOperationCopy — Finder only accepts file-promise drags offered as
/// Copy; Generic gets rejected with operation=0 and the name is never even
/// queried.
const DRAG_OPERATION_COPY: i64 = 1;

/// Shrunk-window hit margin (points). Frames are inset by this much before
/// hit-testing so a pointer exactly on a window edge counts as "outside".
const EDGE_MARGIN: f64 = 2.0;

static REGISTRY: OnceLock<Mutex<PromiseRegistry>> = OnceLock::new();
static CLASS_REGISTERED: Once = Once::new();
/// The single delegate instance (also the dragging source). Created once,
/// lives forever — sessions are short-lived, the object is tiny.
static DELEGATE: OnceLock<usize> = OnceLock::new();

fn registry() -> &'static Mutex<PromiseRegistry> {
    REGISTRY.get_or_init(|| Mutex::new(PromiseRegistry::new()))
}

fn nsstring_to_string(s: id) -> String {
    unsafe {
        let c: *const std::os::raw::c_char = msg_send![s, UTF8String];
        if c.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned()
        }
    }
}

// ───────────────────────────────────────────────────────────────────
// PortalDragDelegate: NSFilePromiseProviderDelegate + NSDraggingSource
// ───────────────────────────────────────────────────────────────────

extern "C" fn source_operation_mask(
    _this: &mut Object,
    _cmd: Sel,
    _session: id,
    _context: usize,
) -> i64 {
    DRAG_OPERATION_COPY
}

extern "C" fn session_ended(
    _this: &mut Object,
    _cmd: Sel,
    _session: id,
    _point: NSPoint,
    _operation: i64,
) {
    // Session over (delivered, cancelled, or dropped nowhere): drop every
    // promise that was never asked to deliver.
    let mut reg = registry().lock().unwrap();
    reg.end_session();
}

extern "C" fn file_name_for_type(_this: &mut Object, _cmd: Sel, provider: id, _type: id) -> id {
    unsafe {
        let name = registry()
            .lock()
            .unwrap()
            .file_name(provider as usize)
            .map(str::to_string);
        match name {
            Some(n) => {
                let s: id = NSString::alloc(nil).init_str(&n);
                let _: () = msg_send![s, autorelease];
                s
            }
            None => nil,
        }
    }
}

// clang's documented Block ABI: the first 16 bytes are isa / flags /
// reserved, then the invoke function pointer taking the block itself as
// its first argument. Used to call Finder's completion handler without a
// block-construction crate.
#[repr(C)]
struct BlockHeader {
    _isa: usize,
    _flags: i32,
    _reserved: i32,
    invoke: unsafe extern "C" fn(*mut BlockHeader, id),
    _descriptor: usize,
}

unsafe fn call_completion(block: id, error: id) {
    if block.is_null() {
        return;
    }
    let header = block as *mut BlockHeader;
    ((*header).invoke)(header, error);
}

/// Download the promised file into `url`'s directory. Shared by the modern
/// (`filePromiseProvider:writePromiseToURL:completionHandler:`) and the
/// deprecated (`provideFileForPromiseProvider:atURL:`) delegate callbacks.
///
/// These delegate callbacks run on the MAIN thread. The actual download is
/// issued here, but the wait for its completion must NOT happen on this
/// thread — blocking it would freeze the egui event loop and kill the
/// progress bar. So this only kicks the transfer off; `done_tx`/`done_rx`
/// let the caller decide where to wait.
unsafe fn deliver_promise(
    provider: id,
    url: id,
) -> Result<oneshot::Receiver<Result<(), String>>, String> {
    let pending = registry().lock().unwrap().take_for_delivery(provider as usize);
    let Some(p) = pending else {
        // Promise was already delivered (or never registered) — nothing to do.
        let (_tx, rx) = oneshot::channel();
        return Ok(rx);
    };
    // The URL Finder hands us is the FULL destination file path
    // (destination dir + the name we returned from fileNameForType) — not
    // just the directory. Use it as-is.
    let path: id = msg_send![url, path];
    let dest = nsstring_to_string(path);

    let (done, rx) = oneshot::channel();
    let sent = if p.spec.is_dir {
        p.cmd_tx.send(SftpCommand::DownloadDirSync {
            remote_dir: p.spec.remote_path.clone(),
            local_dir: dest.clone(),
            done,
        })
    } else {
        p.cmd_tx.send(SftpCommand::DownloadSync {
            remote: p.spec.remote_path.clone(),
            local: dest.clone(),
            done,
        })
    };
    if sent.is_err() {
        return Err(format!("SFTP task channel closed, cannot fulfill {dest}"));
    }
    Ok(rx)
}

/// Modern required delegate method (macOS 13+): deliver asynchronously and
/// signal Finder through the completion block.
extern "C" fn write_promise_to_url(
    _this: &mut Object,
    _cmd: Sel,
    provider: id,
    url: id,
    completion: id,
) {
    unsafe {
        // AppKit only guarantees the completion block stays valid until this
        // callback returns. We deliver it asynchronously, so we must copy it
        // to the heap NOW (proper Block reference counting — `_Block_copy`,
        // not an ObjC `-retain`). The copy is released after invocation.
        extern "C" {
            fn _Block_copy(block: *const std::ffi::c_void) -> *mut std::ffi::c_void;
            fn _Block_release(block: *const std::ffi::c_void);
        }
        let heap_completion = _Block_copy(completion as *const std::ffi::c_void);

        match deliver_promise(provider, url) {
            Ok(rx) => {
                // Wait for the download on a background thread, then hand the
                // completion back to the main queue. The heap block pointer
                // is marshalled as a raw usize (id is not Send).
                let completion_usize = heap_completion as usize;
                std::thread::spawn(move || {
                    let result = rx.blocking_recv();
                    let error: id = match result {
                        Ok(Ok(())) => nil,
                        Ok(Err(msg)) => {
                            eprintln!("drag-out: promise delivery failed: {msg}");
                            let domain: id = NSString::alloc(nil).init_str("PortalSftpDrag");
                            msg_send![
                                class!(NSError),
                                errorWithDomain: domain
                                code: 1isize
                                userInfo: nil
                            ]
                        }
                        Err(_) => {
                            let domain: id = NSString::alloc(nil).init_str("PortalSftpDrag");
                            msg_send![
                                class!(NSError),
                                errorWithDomain: domain
                                code: 2isize
                                userInfo: nil
                            ]
                        }
                    };
                    deliver_completion_on_main(completion_usize as id, error);
                });
            }
            Err(msg) => {
                log::error!("drag-out: {msg}");
                let domain: id = NSString::alloc(nil).init_str("PortalSftpDrag");
                let error: id = msg_send![
                    class!(NSError),
                    errorWithDomain: domain
                    code: 1isize
                    userInfo: nil
                ];
                call_completion(heap_completion as id, error);
                extern "C" {
                    fn _Block_release(block: *const std::ffi::c_void);
                }
                _Block_release(heap_completion);
            }
        }
    }
}

/// Deprecated callback, kept for older macOS where the modern one is absent.
/// Waits on a background thread so the main loop (and progress UI) never
/// blocks.
extern "C" fn provide_file(_this: &mut Object, _cmd: Sel, provider: id, url: id) {
    unsafe {
        match deliver_promise(provider, url) {
            Ok(rx) => {
                std::thread::spawn(move || {
                    let _ = rx.blocking_recv();
                });
            }
            Err(msg) => {
                log::error!("drag-out: {msg}");
            }
        }
    }
}

/// Invoke the retained completion block on the main queue. AppKit requires
/// the completion handler to run there, so we marshal the two ids through a
/// real NSArray and `performSelectorOnMainThread:`, which expects a genuine
/// ObjC object (unlike a raw heap pointer, which it would try to message and
/// crash on).
unsafe fn deliver_completion_on_main(completion: id, error: id) {
    unsafe {
        // Reuse a small NSObject subclass as the trampoline target.
        static TRAMPOLINE: OnceLock<usize> = OnceLock::new();
        static REG: Once = Once::new();

        extern "C" fn invoke_completion(_this: &mut Object, _cmd: Sel, payload: id) {
            unsafe {
                // payload is NSArray of [completion, error]
                let count = NSArray::count(payload);
                if count < 2 {
                    return;
                }
                let completion = NSArray::objectAtIndex(payload, 0);
                let error = NSArray::objectAtIndex(payload, 1);
                call_completion(completion, error);
                // We _Block_copy'd this completion when the callback fired; now
                // release our heap reference.
                extern "C" {
                    fn _Block_release(block: *const std::ffi::c_void);
                }
                _Block_release(completion as *const std::ffi::c_void)
            }
        }

        REG.call_once(|| {
            let decl = ClassDecl::new("PortalCompletionTrampoline", class!(NSObject))
                .expect("trampoline class");
            let mut decl = decl;
            decl.add_method(
                sel!(portalInvoke:),
                invoke_completion as extern "C" fn(&mut Object, Sel, id),
            );
            decl.register();
        });

        let trampoline: id = match TRAMPOLINE.get() {
            Some(&ptr) => ptr as id,
            None => {
                let obj: id = msg_send![class!(PortalCompletionTrampoline), new];
                TRAMPOLINE.set(obj as usize).ok();
                obj
            }
        };

        // Pack completion + error into an NSArray (both are valid objects).
        let arr: id = msg_send![class!(NSMutableArray), array];
        let _: () = msg_send![arr, addObject: completion];
        if error == nil {
            let null: id = msg_send![class!(NSNull), null];
            let _: () = msg_send![arr, addObject: null];
        } else {
            let _: () = msg_send![arr, addObject: error];
        }

        let _: () = msg_send![
            trampoline,
            performSelectorOnMainThread: sel!(portalInvoke:)
            withObject: arr
            waitUntilDone: false
        ];
    }
}

unsafe fn ensure_delegate_class() -> id {
    CLASS_REGISTERED.call_once(|| unsafe {
        let decl = ClassDecl::new("PortalDragDelegate", class!(NSObject))
            .expect("PortalDragDelegate class registration");
        let mut decl = decl;
        decl.add_method(
            sel!(draggingSession:sourceOperationMaskForDraggingContext:),
            source_operation_mask
                as extern "C" fn(&mut Object, Sel, id, usize) -> i64,
        );
        decl.add_method(
            sel!(draggingSession:endedAtPoint:operation:),
            session_ended as extern "C" fn(&mut Object, Sel, id, NSPoint, i64),
        );
        decl.add_method(
            sel!(filePromiseProvider:fileNameForType:),
            file_name_for_type as extern "C" fn(&mut Object, Sel, id, id) -> id,
        );
        decl.add_method(
            sel!(filePromiseProvider:writePromiseToURL:completionHandler:),
            write_promise_to_url as extern "C" fn(&mut Object, Sel, id, id, id),
        );
        decl.add_method(
            sel!(provideFileForPromiseProvider:atURL:),
            provide_file as extern "C" fn(&mut Object, Sel, id, id),
        );
        decl.register();
    });
    let delegate: id = msg_send![class!(PortalDragDelegate), new];
    DELEGATE.set(delegate as usize).ok();
    delegate
}

// ───────────────────────────────────────────────────────────────────
// Drag session
// ───────────────────────────────────────────────────────────────────

/// True when the mouse is outside every visible app window (frames shrunk by
/// EDGE_MARGIN). Uses the native NSEvent mouseLocation because egui's
/// hover_pos stops updating once the pointer leaves the window.
pub(super) fn pointer_outside_app_windows() -> bool {
    unsafe {
        let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        let windows: id = msg_send![NSApp(), windows];
        let count = NSArray::count(windows);
        for i in 0..count {
            let w = NSArray::objectAtIndex(windows, i);
            let visible: bool = msg_send![w, isVisible];
            if !visible {
                continue;
            }
            let frame: NSRect = msg_send![w, frame];
            if frame_contains(frame, mouse) {
                return false;
            }
        }
        true
    }
}

fn frame_contains(frame: NSRect, p: NSPoint) -> bool {
    p.x >= frame.origin.x + EDGE_MARGIN
        && p.x <= frame.origin.x + frame.size.width - EDGE_MARGIN
        && p.y >= frame.origin.y + EDGE_MARGIN
        && p.y <= frame.origin.y + frame.size.height - EDGE_MARGIN
}

/// Badge image for the drag item: small rounded rect with a label. Best
/// effort — if text drawing misbehaves the solid chip is still usable.
unsafe fn make_badge(label: &str) -> id {
    unsafe {
        let size = NSSize::new(140.0, 24.0);
        let image: id = msg_send![class!(NSImage), alloc];
        let image: id = msg_send![image, initWithSize: size];
        let _: () = msg_send![image, lockFocus];

        // Rounded translucent chip
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), size);
        let path: id = msg_send![
            class!(NSBezierPath),
            bezierPathWithRoundedRect: rect
            xRadius: 6.0f64
            yRadius: 6.0f64
        ];
        let _: () = msg_send![path, fill];

        let text: id = NSString::alloc(nil).init_str(label);
        let font: id = msg_send![class!(NSFont), systemFontOfSize: 11.0f64];
        // The attribute key is literally the string "NSFont" —
        // +[NSFont fontAttributeName] throws on modern Foundation.
        let font_key: id = NSString::alloc(nil).init_str("NSFont");
        let attrs: id = msg_send![class!(NSDictionary), dictionaryWithObject: font forKey: font_key];
        let _: () = msg_send![
            text,
            drawAtPoint: NSPoint::new(8.0, 6.0)
            withAttributes: attrs
        ];

        let _: () = msg_send![image, unlockFocus];
        image
    }
}

/// Begin a native file-promise drag session. Must be called on the main
/// thread (i.e. from within the egui frame). Returns true if the session
/// started; on false the caller keeps the in-app drag payload untouched.
pub(super) fn begin_file_promise_drag(
    specs: &[PromiseSpec],
    cmd_tx: UnboundedSender<SftpCommand>,
) -> bool {
    unsafe {
        let pool = NSAutoreleasePool::new(nil);
        let ok = begin_session(specs, cmd_tx);
        let _: () = msg_send![pool, drain];
        ok
    }
}

unsafe fn begin_session(specs: &[PromiseSpec], cmd_tx: UnboundedSender<SftpCommand>) -> bool {
    unsafe {
        // The drag is triggered when the pointer is OUTSIDE every app window
        // (see pointer_outside_app_windows), so there is by definition no
        // window under the mouse. The session just needs *a* view to
        // originate from — keyWindow (or any visible window) works.
        let window: id = msg_send![NSApp(), keyWindow];
        let mut window = window;
        if window == nil {
            window = msg_send![NSApp(), mainWindow];
        }
        if window == nil {
            let windows: id = msg_send![NSApp(), windows];
            let count = NSArray::count(windows);
            for i in 0..count {
                let w = NSArray::objectAtIndex(windows, i);
                let visible: bool = msg_send![w, isVisible];
                if visible {
                    window = w;
                    break;
                }
            }
        }
        if window == nil {
            return false;
        }
        let content_view: id = msg_send![window, contentView];

        let delegate = ensure_delegate_class();

        // Synthesize a left-mouse-drag event at the current pointer position.
        // [NSApp currentEvent] cannot be used here: once the pointer leaves
        // the window, AppKit stops delivering mouse-drags and currentEvent is
        // whatever unrelated event happened last — beginDraggingSession throws
        // unless it gets a real mouse event.
        let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        let frame: NSRect = msg_send![window, frame];
        let loc = NSPoint::new(
            mouse.x - frame.origin.x,
            mouse.y - frame.origin.y,
        );
        let win_num: isize = msg_send![window, windowNumber];
        // NSLeftMouseDragged = 6 (NSEventType: 1=down, 2=up, 5=moved, 6=dragged).
        // Note the runtime signature has an extra `eventNumber:` parameter
        // between context: and clickCount: — the commonly documented one omits
        // it and throws "unrecognized selector".
        let event: id = msg_send![
            class!(NSEvent),
            mouseEventWithType: 6isize
            location: loc
            modifierFlags: 0usize
            timestamp: 0f64
            windowNumber: win_num
            context: nil
            eventNumber: 0isize
            clickCount: 1isize
            pressure: 1.0f32
        ];

        let badge_label = if specs.len() == 1 {
            specs[0].file_name.clone()
        } else {
            format!("{} items", specs.len())
        };
        let badge = make_badge(&badge_label);

        // Center the drag badge on the cursor (content-view base coords,
        // bottom-left origin).
        let badge_frame = NSRect::new(
            NSPoint::new(loc.x - 70.0, loc.y - 12.0),
            NSSize::new(140.0, 24.0),
        );

        let items: id = msg_send![class!(NSMutableArray), array];
        for spec in specs {
            let file_type: id = NSString::alloc(nil).init_str("public.data");
            let provider: id = msg_send![class!(NSFilePromiseProvider), alloc];
            let provider: id = msg_send![provider, initWithFileType: file_type delegate: delegate];
            let item: id = msg_send![class!(NSDraggingItem), alloc];
            let item: id = msg_send![item, initWithPasteboardWriter: provider];
            let _: () = msg_send![item, setDraggingFrame: badge_frame contents: badge];
            let _: () = msg_send![items, addObject: item];
            registry()
                .lock()
                .unwrap()
                .insert(provider as usize, spec.clone(), cmd_tx.clone());
        }

        let session: id = msg_send![
            content_view,
            beginDraggingSessionWithItems: items
            event: event
            source: delegate
        ];
        if session == nil {
            registry().lock().unwrap().end_session();
            return false;
        }
        true
    }
}
