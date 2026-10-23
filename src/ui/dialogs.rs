use adw::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Suggested,
    Destructive,
}

/// Shows a message dialog and resolves to the id of the chosen response. `cancel` is
/// returned when the dialog is dismissed (Esc / close).
pub async fn ask(
    parent: &impl IsA<gtk::Window>,
    heading: &str,
    body: &str,
    responses: &[(&str, &str, Tone)],
    default: &str,
    cancel: &str,
) -> String {
    let dialog = adw::MessageDialog::new(Some(parent), Some(heading), Some(body));
    for (id, label, tone) in responses {
        dialog.add_response(id, label);
        match tone {
            Tone::Suggested => dialog.set_response_appearance(id, adw::ResponseAppearance::Suggested),
            Tone::Destructive => dialog.set_response_appearance(id, adw::ResponseAppearance::Destructive),
            Tone::Normal => {}
        }
    }
    dialog.set_default_response(Some(default));
    dialog.set_close_response(cancel);
    dialog.choose_future().await.to_string()
}

pub async fn notify(parent: &impl IsA<gtk::Window>, heading: &str, body: &str) {
    ask(parent, heading, body, &[("ok", "OK", Tone::Suggested)], "ok", "ok").await;
}
