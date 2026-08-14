#ifndef SWIFT_TERM_BRIDGE_H
#define SWIFT_TERM_BRIDGE_H

#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

// Callback types
// input_callback(context, data_ptr, data_len) — called when user types
typedef void (*SwiftTermInputCallback)(void *context, const void *data, size_t len);
// activity_callback(context, active) — called on activity state changes
typedef void (*SwiftTermActivityCallback)(void *context, bool active);

// Create a terminal view as subview of the given NSView (content view of window)
// Returns true on success
bool swift_term_create(
    const char *session_id,
    void *parent_ns_view,
    double x, double y, double width, double height,
    double font_size,
    const char *font_family,  // NULL for default
    SwiftTermInputCallback input_callback,
    SwiftTermActivityCallback activity_callback,
    void *callback_context
);

// Feed PTY output data to the terminal for rendering
void swift_term_feed(const char *session_id, const void *data, size_t len);

// Update the terminal view's frame position and size
void swift_term_resize(const char *session_id, double x, double y, double width, double height);

// Get the current terminal dimensions in cells (after a resize)
void swift_term_get_size(const char *session_id, uint16_t *out_cols, uint16_t *out_rows);

// Show or hide the terminal view
void swift_term_set_visible(const char *session_id, bool visible);

// Write data to the terminal (for startup commands injected from Rust side)
void swift_term_write(const char *session_id, const char *text);

// Destroy the terminal view and clean up
void swift_term_destroy(const char *session_id);

// MARK: - Notifications (UNUserNotificationCenter)

// action_callback(context, identifier) — called when the user clicks a notification
typedef void (*WBNotificationActionCallback)(void *context, const char *identifier);

// Whether notifications can work at all. False when the executable has no bundle
// identifier (i.e. `tauri dev`), where UNUserNotificationCenter would raise.
bool wb_notifications_available(void);

// Install the delegate and request authorization. Call during app setup, before the
// app finishes launching, or click responses are dropped.
bool wb_notifications_init(WBNotificationActionCallback callback, void *context);

// Post a notification. `identifier` is the replace key and is returned on click.
bool wb_notification_send(const char *identifier, const char *title, const char *body);

#endif
