use relm4::gtk::{ButtonsType, MessageDialog, MessageType, Window};

pub fn build_fatal_error_dialog(msg: &str, window: Option<Window>) -> MessageDialog {
    build_error_dialog("Fatal error", msg, window)
}

pub fn build_error_dialog(title: &str, msg: &str, window: Option<Window>) -> MessageDialog {
    let mut b = MessageDialog::builder();
    if let Some(w) = window {
        b = b.transient_for(&w);
    }

    b.title(title)
        .message_type(MessageType::Error)
        .buttons(ButtonsType::Ok)
        .secondary_text(msg)
        .modal(true)
        .build()
}
