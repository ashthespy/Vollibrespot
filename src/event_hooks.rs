use librespot::core::session::Session;
use librespot::core::events::Event;

pub fn handle_events(event: Event, session: Session) {
    match event {
        Event::SessionActive { became_active_at } => {
            info!("Session [{}]", session.session_id());
            info!("Active at: {:?}", became_active_at);
        }
        _ => {
            info!("Matched: {:?}", event);
        }
    }
}
