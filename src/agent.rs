//! The macOS side: run as a background agent and receive URLs.
//!
//! LaunchServices can deliver a URL two ways depending on when AppKit installs
//! its own Apple Event handlers, so we cover both:
//!
//! 1. an `NSAppleEventManager` handler for `kInternetEventClass`/`kAEGetURL`, and
//! 2. the `NSApplicationDelegate` `application:openURLs:` callback.
//!
//! Only one of them ever fires for a given event, so this is redundancy, not
//! double handling.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
use objc2_core_services::{keyDirectObject, typeFileURL, AEEventClass, AEEventID};
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleEventManager, NSArray, NSObject, NSObjectProtocol, NSURL,
};

// AppleEvents.h: both of these are the four-character code 'GURL'.
const K_INTERNET_EVENT_CLASS: AEEventClass = 0x4755_524c;
const K_AE_GET_URL: AEEventID = 0x4755_524c;

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements and `Handler` has no
    // `Drop` implementation.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UrwhereURLHandler"]
    struct Handler;

    unsafe impl NSObjectProtocol for Handler {}

    unsafe impl NSApplicationDelegate for Handler {
        #[unsafe(method(application:openURLs:))]
        fn application_open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            for url in urls.iter() {
                if let Some(text) = url.absoluteString() {
                    crate::route::handle_url(&text.to_string());
                }
            }
        }
    }

    impl Handler {
        #[unsafe(method(handleGetURL:withReplyEvent:))]
        fn handle_get_url(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let Some(descriptor) = event.paramDescriptorForKeyword(keyDirectObject) else {
                return;
            };
            let url = if descriptor.descriptorType() == typeFileURL {
                descriptor
                    .fileURLValue()
                    .and_then(|url| url.absoluteString())
                    .map(|text| text.to_string())
            } else {
                descriptor.stringValue().map(|text| text.to_string())
            };
            if let Some(url) = url {
                crate::route::handle_url(&url);
            }
        }
    }
);

impl Handler {
    fn new(marker: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(marker).set_ivars(());
        // SAFETY: `Handler` is a freshly allocated `NSObject` subclass with its
        // ivars set, so `init` is the correct initialiser.
        unsafe { msg_send![super(this), init] }
    }
}

/// Run forever as a URL handler agent.
pub fn run() -> ! {
    let marker = MainThreadMarker::new().expect("urwhere must run on the main thread");
    let app = NSApplication::sharedApplication(marker);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let handler = Handler::new(marker);
    app.setDelegate(Some(ProtocolObject::from_ref(&*handler)));

    // SAFETY: `handler` outlives the run loop, and the selector matches the
    // method defined above.
    unsafe {
        let handler_object: &AnyObject = &*(Retained::as_ptr(&handler) as *const AnyObject);
        NSAppleEventManager::sharedAppleEventManager()
            .setEventHandler_andSelector_forEventClass_andEventID(
                handler_object,
                sel!(handleGetURL:withReplyEvent:),
                K_INTERNET_EVENT_CLASS,
                K_AE_GET_URL,
            );
    }

    app.run();
    std::process::exit(0);
}
