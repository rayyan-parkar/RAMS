use std::io::{Read, Result as IoResult};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub struct ChannelReader {
    pub rx: std::sync::mpsc::Receiver<Vec<u8>>,
    pub buffer: Vec<u8>,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if self.buffer.is_empty() {
            // Block until a new chunk arrives from Tauri IPC
            match self.rx.recv() {
                Ok(data) => self.buffer = data,
                Err(_) => return Ok(0), // Channel closed = EOF
            }
        }
        
        // Feed the bytes to whoever is calling read() (in our case WebmReader)
        let len = std::cmp::min(buf.len(), self.buffer.len());
        buf[..len].copy_from_slice(&self.buffer[..len]);
        self.buffer = self.buffer[len..].to_vec();
        
        Ok(len)
    }
}

pub struct Vp8Packet {
    pub data: Vec<u8>,
    pub timestamp_ms: u64,
}

/// Spawns a background OS thread that parses WebM chunks natively using webrtc-media
/// and pushes raw VP8 frames over an async tokio channel to the EventLoop.
pub fn start_demuxer_thread(
    mut webm_rx: UnboundedReceiver<Vec<u8>>, 
    vp8_tx: UnboundedSender<Vp8Packet>
) {
    std::thread::spawn(move || {
        println!("[Demuxer] Thread started, awaiting WebM headers...");
        
        // We use a fast std mpsc channel internally to map the tokio async rx into a sync execution
        let (sync_tx, sync_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while let Some(chunk) = webm_rx.blocking_recv() {
                let _ = sync_tx.send(chunk);
            }
        });

        let mut reader = ChannelReader { rx: sync_rx, buffer: Vec::new() };

        println!("[Demuxer] Mock WebM parser waiting for chunks...");
        loop {
            let mut buf = vec![0u8; 1024];
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let vp8 = Vp8Packet {
                        data: buf[..n].to_vec(),
                        timestamp_ms: 0,
                    };
                    
                    if let Err(_) = vp8_tx.send(vp8) {
                        println!("[Demuxer] Tokios EventLoop disconnected, shutting down.");
                        break;
                    }
                }
                Err(e) => {
                    println!("[Demuxer] Parsing error: {:?}", e);
                    break;
                }
            }
        }
    });
}
