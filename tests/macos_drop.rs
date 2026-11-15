#[cfg(target_os = "macos")]
#[path = "../src/macos.rs"]
#[allow(dead_code)]
mod macos;
#[cfg(not(target_os = "macos"))]
fn main() {}
#[cfg(target_os = "macos")]
fn main() {
    use objc2::{
        DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
    };
    use objc2_app_kit::*;
    use objc2_foundation::*;
    struct Info {
        pb: Retained<NSPasteboard>,
    }
    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = Info]
        struct Drag;
        unsafe impl NSObjectProtocol for Drag {}
        impl Drag {
            #[unsafe(method(draggingPasteboard))]
            fn pasteboard(&self) -> &NSPasteboard {
                &self.ivars().pb
            }
            #[unsafe(method(draggingLocation))]
            fn location(&self) -> NSPoint {
                NSPoint::new(10., 10.)
            }
        }
    );
    struct PromiseState {
        names: Retained<NSArray<NSString>>,
        empty: Retained<NSArray<NSString>>,
        requested: std::cell::Cell<bool>,
        deferred: bool,
    }
    define_class!(
        #[unsafe(super(NSFilePromiseReceiver))]
        #[thread_kind = MainThreadOnly]
        #[ivars = PromiseState]
        struct Promise;
        unsafe impl NSObjectProtocol for Promise {}
        impl Promise {
            #[unsafe(method(fileNames))]
            fn names(&self) -> &NSArray<NSString> { if self.ivars().requested.get() { &self.ivars().names } else { &self.ivars().empty } }
            #[unsafe(method(receivePromisedFilesAtDestination:options:operationQueue:reader:))]
            fn receive(&self, destination: &NSURL, _: &NSDictionary, queue: &NSOperationQueue, reader: &block2::DynBlock<dyn Fn(std::ptr::NonNull<NSURL>, *mut NSError)>) {
                let path = std::path::PathBuf::from(destination.path().unwrap().to_string()).join("promised.png");
                image::RgbaImage::new(3,3).save(&path).unwrap();
                let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                // Exercise completion before the request returns and names become available.
                if self.ivars().deferred {
                    let reader = reader.copy();
                    let operation = block2::RcBlock::new(move || reader.call((std::ptr::NonNull::from(&*url), std::ptr::null_mut())));
                    unsafe { queue.addOperationWithBlock(&operation); }
                } else {
                    reader.call((std::ptr::NonNull::from(&*url), std::ptr::null_mut()));
                }
                self.ivars().requested.set(true);
            }
        }
    );
    let mtm = MainThreadMarker::new().unwrap();
    let _app = NSApplication::sharedApplication(mtm);
    let rect = NSRect::new(NSPoint::new(0., 0.), NSSize::new(300., 300.));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect,
            NSWindowStyleMask::Titled,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    let view = window.contentView().unwrap();
    let bridge = macos::DropBridge::from_view(view.clone(), eframe::egui::Context::default());
    bridge.set_zones(
        vec![(
            "test-host".into(),
            eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0., 0.),
                eframe::egui::pos2(300., 300.),
            ),
        )],
        1.,
    );
    let pb = NSPasteboard::pasteboardWithUniqueName();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("test.png");
    image::RgbaImage::new(2, 2).save(&path).unwrap();
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    assert!(pb.writeObjects(&NSArray::from_slice(&[ProtocolObject::from_ref(&*url)])));
    let allocated = Drag::alloc(mtm).set_ivars(Info { pb: pb.clone() });
    let drag: Retained<Drag> = unsafe { msg_send![super(allocated), init] };
    let info = unsafe { &*(&*drag as *const Drag).cast::<ProtocolObject<dyn NSDraggingInfo>>() };
    let entered: NSDragOperation = unsafe { msg_send![&*view, draggingEntered: info] };
    assert_eq!(entered, NSDragOperation::Copy);
    assert_eq!(bridge.drag_hover(), (true, Some("test-host".into())));
    let _: () = unsafe { msg_send![&*view, draggingExited: info] };
    assert_eq!(bridge.drag_hover(), (false, None));
    let _: NSDragOperation = unsafe { msg_send![&*view, draggingUpdated: info] };
    assert_eq!(bridge.drag_hover(), (true, Some("test-host".into())));
    let accepted: bool = unsafe { msg_send![&*view, performDragOperation: info] };
    assert!(accepted);
    assert_eq!(bridge.drag_hover(), (false, None));
    let result = bridge.receive().pop().unwrap().unwrap();
    assert_eq!(result.host, "test-host");
    assert_eq!(result.paths, vec![path]);

    for deferred in [false, true] {
        let allocated = Promise::alloc(mtm).set_ivars(PromiseState {
            deferred,
            names: NSArray::from_slice(&[&*NSString::from_str("promised.png")]),
            empty: NSArray::new(),
            requested: std::cell::Cell::new(false),
        });
        let promise: Retained<Promise> = unsafe { msg_send![super(allocated), init] };
        let object: &objc2::runtime::AnyObject = &promise;
        let promises = NSArray::from_slice(&[object]);
        bridge
            .view
            .accept_promises("test-host".into(), promises)
            .unwrap();
        let start = std::time::Instant::now();
        let result = loop {
            if let Some(result) = bridge.receive().pop() {
                break result.unwrap();
            }
            assert!(start.elapsed().as_secs() < 10, "Promise was not delivered");
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        };
        assert_eq!(result.host, "test-host");
        assert_eq!(result.paths.len(), 1);
        assert_eq!(image::open(&result.paths[0]).unwrap().width(), 3);
    }
    println!("Native AppKit file and synchronous/asynchronous screenshot-promise drops passed");
}
