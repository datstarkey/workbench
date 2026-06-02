use tauri::{
    menu::{AboutMetadataBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    AppHandle, Emitter,
};

const CHECK_FOR_UPDATES_ID: &str = "check-for-updates";
const SETTINGS_ID: &str = "settings";

pub fn build(app: &AppHandle) -> anyhow::Result<()> {
    let app_menu = SubmenuBuilder::new(app, "Workbench")
        .about(Some(
            AboutMetadataBuilder::new()
                .name(Some("Workbench"))
                .build(),
        ))
        .separator()
        .item(
            &MenuItemBuilder::with_id(CHECK_FOR_UPDATES_ID, "Check for Updates…")
                .build(app)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id(SETTINGS_ID, "Settings…")
                .accelerator("CmdOrCtrl+,")
                .build(app)?,
        )
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .close_window()
        .separator()
        .build()?;

    let menu = MenuBuilder::new(app)
        .item(&app_menu)
        .item(&edit_menu)
        .item(&window_menu)
        .build()?;

    app.set_menu(menu)?;

    let handle = app.clone();
    app.on_menu_event(move |_app, event| {
        let id = event.id().0.as_str();
        match id {
            CHECK_FOR_UPDATES_ID => {
                let _ = handle.emit("menu:check-for-updates", ());
            }
            SETTINGS_ID => {
                let _ = handle.emit("menu:open-settings", ());
            }
            _ => {}
        }
    });

    Ok(())
}
