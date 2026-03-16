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

        let config = RtcConfig::default();
        let rams_client = Rtc::new();

        self.rtc_client = rams_client;
        Ok(self)
    }
}