//! AppKit receives file promises before routing materialized files to the upload queue.
use eframe::egui;
use objc2::{
    ClassType, DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject,
};
use objc2_app_kit::*;
use objc2_foundation::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
pub struct Drop {
    pub host: String,
    pub paths: Vec<PathBuf>,
    pub temporary: tempfile::TempDir,
}
struct PendingPromise {
    _receivers: Retained<NSArray>,
    _queue: Retained<NSOperationQueue>,
    done: Arc<AtomicBool>,
}
pub(crate) struct Shared {
    dragging: Cell<bool>,
    hovered_host: RefCell<Option<String>>,
    pending: RefCell<Vec<PendingPromise>>,
    original: Retained<NSView>,
    zones: RefCell<Vec<(String, egui::Rect)>>,
    tx: mpsc::Sender<Result<Drop, String>>,
    ctx: egui::Context,
}
define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Shared]
    pub(crate) struct DropView;
    unsafe impl NSObjectProtocol for DropView {}
    unsafe impl NSDraggingDestination for DropView {
        #[unsafe(method(draggingEntered:))]
        fn entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.operation(sender)
        }
        #[unsafe(method(draggingUpdated:))]
        fn updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.operation(sender)
        }
        #[unsafe(method(prepareForDragOperation:))]
        fn prepare(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            self.host(sender).is_some()
        }
        #[unsafe(method(performDragOperation:))]
        fn perform(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            self.end_drag();
            match self.accept(sender) {
                Ok(()) => true,
                Err(e) => {
                    let _ = self.ivars().tx.send(Err(e));
                    self.ivars().ctx.request_repaint();
                    false
                }
            }
        }
    }
);
impl DropView {
    fn host(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> Option<String> {
        let p = self
            .ivars()
            .original
            .convertPoint_fromView(sender.draggingLocation(), None);
        let scale = self
            .ivars()
            .original
            .window()
            .map(|w| w.backingScaleFactor() as f32)
            .unwrap_or(1.0)
            / self.ivars().ctx.pixels_per_point();
        let pos = egui::pos2(p.x as f32 * scale, p.y as f32 * scale);
        self.ivars()
            .zones
            .borrow()
            .iter()
            .find(|(_, r)| r.contains(pos))
            .map(|(id, _)| id.clone())
    }
    fn operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        let host = self.host(sender);
        let changed =
            !self.ivars().dragging.replace(true) || *self.ivars().hovered_host.borrow() != host;
        *self.ivars().hovered_host.borrow_mut() = host.clone();
        if changed {
            self.ivars().ctx.request_repaint();
        }
        if host.is_some() {
            NSDragOperation::Copy
        } else {
            NSDragOperation::None
        }
    }
    fn end_drag(&self) {
        self.ivars().hovered_host.borrow_mut().take();
        if self.ivars().dragging.replace(false) {
            self.ivars().ctx.request_repaint();
        }
    }
    pub(crate) fn accept_promises(
        &self,
        host: String,
        promises: Retained<NSArray>,
    ) -> Result<(), String> {
        let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(&temp.path().to_string_lossy()));
        struct Batch {
            temp: Option<tempfile::TempDir>,
            expected: usize,
            completed: usize,
            accepting: bool,
            paths: Vec<PathBuf>,
            error: Option<String>,
        }
        let batch = Arc::new(Mutex::new(Batch {
            temp: Some(temp),
            expected: 0,
            completed: 0,
            accepting: true,
            paths: vec![],
            error: None,
        }));
        let queue = NSOperationQueue::new();
        let done = Arc::new(AtomicBool::new(false));
        for promise in promises.iter() {
            let done = done.clone();
            let callback_batch = batch.clone();
            let tx = self.ivars().tx.clone();
            let ctx = self.ivars().ctx.clone();
            let host = host.clone();
            let block =
                block2::RcBlock::new(move |url: std::ptr::NonNull<NSURL>, error: *mut NSError| {
                    let mut b = callback_batch.lock().unwrap();
                    if !error.is_null() {
                        b.error = Some(unsafe { (&*error).localizedDescription().to_string() });
                    } else if let Some(path) = unsafe { url.as_ref() }.path() {
                        b.paths.push(PathBuf::from(path.to_string()));
                    }
                    b.completed += 1;
                    if !b.accepting && b.completed == b.expected {
                        let result = if let Some(e) = b.error.take() {
                            Err(e)
                        } else {
                            Ok(Drop {
                                host: host.clone(),
                                paths: std::mem::take(&mut b.paths),
                                temporary: b.temp.take().unwrap(),
                            })
                        };
                        done.store(true, Ordering::Release);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    }
                });
            unsafe {
                promise
                    .downcast_ref::<NSFilePromiseReceiver>()
                    .unwrap()
                    .receivePromisedFilesAtDestination_options_operationQueue_reader(
                        &url,
                        &NSDictionary::new(),
                        &queue,
                        &block,
                    );
            }
            // AppKit only populates fileNames after receivePromisedFiles is called.
            batch.lock().unwrap().expected += promise
                .downcast_ref::<NSFilePromiseReceiver>()
                .unwrap()
                .fileNames()
                .len()
                .max(1);
        }
        let mut b = batch.lock().unwrap();
        b.accepting = false;
        if b.completed == b.expected {
            let result = if let Some(e) = b.error.take() {
                Err(e)
            } else {
                Ok(Drop {
                    host,
                    paths: std::mem::take(&mut b.paths),
                    temporary: b.temp.take().unwrap(),
                })
            };
            done.store(true, Ordering::Release);
            let _ = self.ivars().tx.send(result);
            self.ivars().ctx.request_repaint();
        }
        self.ivars().pending.borrow_mut().push(PendingPromise {
            _receivers: promises,
            _queue: queue,
            done,
        });
        Ok(())
    }

    fn accept(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> Result<(), String> {
        let host = self.host(sender).ok_or("Drop onto a host card")?;
        let pb = sender.draggingPasteboard();
        // The requested class constrains every object returned by AppKit.
        let promises = unsafe {
            pb.readObjectsForClasses_options(
                &NSArray::from_slice(&[NSFilePromiseReceiver::class()]),
                None,
            )
        };
        if let Some(promises) = promises.filter(|p| !p.is_empty()) {
            return self.accept_promises(host, promises);
        }
        let urls = unsafe {
            pb.readObjectsForClasses_options(&NSArray::from_slice(&[NSURL::class()]), None)
        }
        .ok_or("Drop an image file")?;
        let paths = urls
            .iter()
            .filter_map(|o| {
                let u = o.downcast_ref::<NSURL>()?;
                if u.isFileURL() { u.path() } else { None }
            })
            .map(|p| PathBuf::from(p.to_string()))
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return Err("Drop an image file".into());
        }
        let _ = self.ivars().tx.send(Ok(Drop {
            host,
            paths,
            temporary: tempfile::tempdir().map_err(|e| e.to_string())?,
        }));
        self.ivars().ctx.request_repaint();
        Ok(())
    }
}
pub struct DropBridge {
    pub(crate) view: Retained<DropView>,
    rx: mpsc::Receiver<Result<Drop, String>>,
}
impl DropBridge {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let RawWindowHandle::AppKit(handle) = cc.window_handle().expect("AppKit handle").as_raw()
        else {
            unreachable!()
        };
        let original =
            unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()).unwrap() };
        Self::from_view(original, cc.egui_ctx.clone())
    }
    pub(crate) fn from_view(original: Retained<NSView>, ctx: egui::Context) -> Self {
        let mtm = MainThreadMarker::new().unwrap();
        let window = original.window().unwrap();
        let rect = original.frame();
        let (tx, rx) = mpsc::channel();
        let allocated = DropView::alloc(mtm).set_ivars(Shared {
            dragging: Cell::new(false),
            hovered_host: RefCell::new(None),
            pending: RefCell::new(vec![]),
            original: original.clone(),
            zones: RefCell::new(vec![]),
            tx,
            ctx,
        });
        let view: Retained<DropView> = unsafe { msg_send![super(allocated), initWithFrame: rect] };
        let types = NSFilePromiseReceiver::readableDraggedTypes().mutableCopy();
        types.addObject(&NSString::from_str("public.file-url"));
        original.registerForDraggedTypes(&types);
        window.unregisterDraggedTypes();
        install_drop_methods(&original, view.clone());
        Self { view, rx }
    }
    pub fn drag_hover(&self) -> (bool, Option<String>) {
        (
            self.view.ivars().dragging.get(),
            self.view.ivars().hovered_host.borrow().clone(),
        )
    }
    pub fn set_zones(&self, zones: Vec<(String, egui::Rect)>, _: f32) {
        *self.view.ivars().zones.borrow_mut() = zones;
    }
    pub fn receive(&self) -> Vec<Result<Drop, String>> {
        self.view
            .ivars()
            .pending
            .borrow_mut()
            .retain(|p| !p.done.load(Ordering::Acquire));
        self.rx.try_iter().collect()
    }
}

thread_local! { static RECEIVER: RefCell<Option<Retained<DropView>>> = const { RefCell::new(None) }; }
fn install_drop_methods(original: &NSView, receiver: Retained<DropView>) {
    use objc2::{
        runtime::{AnyObject, Bool, ClassBuilder, Sel},
        sel,
    };
    extern "C-unwind" fn entered(
        _: &AnyObject,
        _: Sel,
        info: &ProtocolObject<dyn NSDraggingInfo>,
    ) -> NSDragOperation {
        RECEIVER.with(|r| r.borrow().as_ref().unwrap().operation(info))
    }
    extern "C-unwind" fn perform(
        _: &AnyObject,
        _: Sel,
        info: &ProtocolObject<dyn NSDraggingInfo>,
    ) -> Bool {
        RECEIVER.with(|r| {
            r.borrow()
                .as_ref()
                .unwrap()
                .perform(sel!(performDragOperation:), info)
        })
    }
    extern "C-unwind" fn exited(_: &AnyObject, _: Sel, _: *mut AnyObject) {
        RECEIVER.with(|r| r.borrow().as_ref().unwrap().end_drag());
    }
    // Preserve winit's content view and ivar layout: winit accesses it directly during key events.
    let mut builder = ClassBuilder::new(c"SSHawtyDropContentView", original.class()).unwrap();
    unsafe {
        builder.add_method(
            sel!(draggingEntered:),
            entered as extern "C-unwind" fn(_, _, _) -> _,
        );
        builder.add_method(
            sel!(draggingUpdated:),
            entered as extern "C-unwind" fn(_, _, _) -> _,
        );
        builder.add_method(
            sel!(performDragOperation:),
            perform as extern "C-unwind" fn(_, _, _) -> _,
        );
        builder.add_method(
            sel!(draggingExited:),
            exited as extern "C-unwind" fn(_, _, _),
        );
        builder.add_method(
            sel!(draggingEnded:),
            exited as extern "C-unwind" fn(_, _, _),
        );
        AnyObject::set_class(original, builder.register());
    }
    RECEIVER.with(|r| *r.borrow_mut() = Some(receiver));
}
