import Foundation
import UserNotifications

// UNUserNotificationCenter is the only notification API macOS still delivers on.
// `tauri-plugin-notification` routes macOS through notify-rust -> mac-notification-sys,
// which posts to NSUserNotificationCenter — deprecated in macOS 11 and inert since.
// It also discards the delivery error, so the failure never reaches JavaScript.
//
// This API refuses to register an app that isn't signed and bundled, which is why it
// only became usable once the app shipped with a Developer ID signature.

public typealias WBNotificationActionCallback = @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<CChar>?
) -> Void

private final class NotificationDelegate: NSObject, UNUserNotificationCenterDelegate {
    var callback: WBNotificationActionCallback?
    var context: UnsafeMutableRawPointer?

    /// Without this, macOS suppresses banners while the app is frontmost. The caller
    /// already decides whether a notification is warranted (it suppresses only when the
    /// user is looking at the pane in question), so always present.
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .sound])
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let identifier = response.notification.request.identifier
        if let callback {
            identifier.withCString { callback(context, $0) }
        }
        completionHandler()
    }
}

private let delegate = NotificationDelegate()
private let stateLock = NSLock()
private var authorized = false

private func setAuthorized(_ value: Bool) {
    stateLock.lock()
    defer { stateLock.unlock() }
    authorized = value
}

private func isAuthorized() -> Bool {
    stateLock.lock()
    defer { stateLock.unlock() }
    return authorized
}

/// `UNUserNotificationCenter.current()` raises an uncatchable Objective-C exception when
/// the executable has no bundle identifier — which is exactly how `tauri dev` runs, from
/// target/debug rather than a .app. Every entry point below is gated on this.
private func isBundled() -> Bool {
    Bundle.main.bundleIdentifier != nil
}

@_cdecl("wb_notifications_available")
public func wb_notifications_available() -> Bool {
    isBundled()
}

/// Installs the delegate and requests authorization. Must run before the app finishes
/// launching, or macOS drops click responses for notifications delivered early.
/// Returns false when unavailable (unbundled); authorization itself resolves async.
@_cdecl("wb_notifications_init")
public func wb_notifications_init(
    callback: WBNotificationActionCallback?,
    context: UnsafeMutableRawPointer?
) -> Bool {
    guard isBundled() else { return false }

    delegate.callback = callback
    delegate.context = context

    let center = UNUserNotificationCenter.current()
    center.delegate = delegate
    center.requestAuthorization(options: [.alert, .sound]) { granted, error in
        if let error {
            NSLog("[workbench] notification authorization failed: \(error.localizedDescription)")
        }
        setAuthorized(granted)
    }
    return true
}

/// Posts a notification. `identifier` doubles as the replace key — reusing one replaces
/// the existing banner rather than stacking, and comes back on click so the caller can
/// route to the right pane.
@_cdecl("wb_notification_send")
public func wb_notification_send(
    identifier: UnsafePointer<CChar>?,
    title: UnsafePointer<CChar>?,
    body: UnsafePointer<CChar>?
) -> Bool {
    guard isBundled(), isAuthorized(),
          let identifier, let title, let body
    else { return false }

    let content = UNMutableNotificationContent()
    content.title = String(cString: title)
    content.body = String(cString: body)
    content.sound = .default

    let request = UNNotificationRequest(
        identifier: String(cString: identifier),
        content: content,
        trigger: nil  // nil delivers immediately
    )

    UNUserNotificationCenter.current().add(request) { error in
        if let error {
            NSLog("[workbench] notification delivery failed: \(error.localizedDescription)")
        }
    }
    return true
}
