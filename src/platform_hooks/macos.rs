//! Startup hooks — macOS: Dock presence + frontmost activation.

/// Set the macOS activation policy to Regular so the app appears in the
/// Dock, and pull it to the front over other apps.
pub fn activate_gui() {
    use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicy};
    use cocoa::base::{id, YES};

    unsafe {
        let app: id = NSApp();
        app.setActivationPolicy_(
            NSApplicationActivationPolicy::NSApplicationActivationPolicyRegular,
        );
        app.activateIgnoringOtherApps_(YES);
    }
}
