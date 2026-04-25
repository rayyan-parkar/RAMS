use std::time::Instant;
use str0m::media::{Direction, MediaKind};
use str0m::Rtc;
use tauri::Url;
use std::net::TcpListener;

struct Rams{
    rtc_client : Rtc,
    state: RamsConnection
}

enum RamsError {
    IceFailure,
    PortFailure(String),
}

pub enum RamsConnection {
    New,
    LocalOfferCreated,
    WaitingForAnswer,
    Connecting,
    Connected,
    Failed
}
// You look quite wonderful today
impl Rams {
    pub fn new() ->  Self {
        let client = Rtc::new(Instant::now());
        Self {
            rtc_client : client,
            state : RamsConnection::New
        }
    }
    pub fn quick_connect(&mut self, connect_to : Url) ->Result<&mut Self, RamsError> {
        let mut changes = self.rtc_client.sdp_api();
        let mid_audio = changes.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
        let mid_video = changes.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);

        let (offer, pending) = changes.apply().unwrap();
        self.state = RamsConnection::WaitingForAnswer;

        // Make a listener to listen on a random port decided by the OS
        let _listener : TcpListener = TcpListener::bind("127.0.0.1:0").map_err( |e| RamsError::PortFailure(e.to_string()))?;

        Ok(self)
    }
}