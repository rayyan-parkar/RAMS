use std::time::Instant;
use str0m::media::{Direction, MediaKind};
use str0m::Rtc;
use str0m::RtcConfig;

struct Rams{
    rtc_client : Rtc,
}

enum RamsError {
    IceFailure
}

impl Rams {
    pub fn quick_connect(mut self) ->Result<Rams, RamsError> {

        let mut rtc = Rtc::new(Instant::now());
        let mut changes = rtc.sdp_api();

        let audio = changes.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
        let video = changes.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);

        let (offer, pending) = changes.apply().unwrap();

        Ok(self)
    }
}