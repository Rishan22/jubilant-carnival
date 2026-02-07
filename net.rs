use std::{collections::VecDeque, net::UdpSocket, time::{Duration, Instant}};
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
struct Snapshot { tick: u32, entities: Vec<(u32, [f32; 3], [f32; 4])> }

#[derive(Serialize, Deserialize)]
struct Packet { seq: u32, ack: u32, ack_bits: u32, cmds: Vec<(u32, u8, u64)>, snap: Option<Snapshot> }

struct NetLayer {
    sock: UdpSocket, snapshots: VecDeque<Snapshot>, tick: u32, seq: u32, ack: u32, ack_bits: u32,
    cmd_queue: Vec<(u32, u8)>, last_snap: Option<Snapshot>, interp_delay: u32,
}

impl NetLayer {
    fn new(addr: &str, is_server: bool) -> Self {
        let sock = UdpSocket::bind(addr).unwrap();
        sock.set_nonblocking(true).unwrap();
        Self { sock, snapshots: VecDeque::with_capacity(64), tick: 0, seq: 0, ack: 0, 
               ack_bits: 0, cmd_queue: Vec::new(), last_snap: None, interp_delay: 3 }
    }

    fn send(&mut self, peer: &str, cmds: &[(u32, u8)], snap: Option<Snapshot>) {
        let pkt = Packet { seq: self.seq, ack: self.ack, ack_bits: self.ack_bits,
                          cmds: cmds.iter().map(|(id, i)| (*id, *i, 0)).collect(), snap };
        self.sock.send_to(&bincode::serialize(&pkt).unwrap(), peer).ok();
        self.seq += 1;
    }

    fn recv(&mut self) -> Option<Packet> {
        let mut buf = [0u8; 4096];
        self.sock.recv(&mut buf).ok().and_then(|n| bincode::deserialize(&buf[..n]).ok())
            .map(|pkt: Packet| { 
                if pkt.seq > self.ack { self.ack_bits = (self.ack_bits << (pkt.seq - self.ack)) | 1; self.ack = pkt.seq; }
                else if pkt.seq < self.ack { self.ack_bits |= 1 << (self.ack - pkt.seq); }
                if let Some(s) = &pkt.snap { self.snapshots.push_back(s.clone()); if self.snapshots.len() > 64 { self.snapshots.pop_front(); }}
                pkt
            })
    }

    fn interpolate(&self, target_tick: u32) -> Option<Vec<(u32, [f32; 3], [f32; 4])>> {
        let s: Vec<_> = self.snapshots.iter().collect();
        s.windows(2).find(|w| w[0].tick <= target_tick && target_tick < w[1].tick).map(|w| {
            let a = (target_tick - w[0].tick) as f32 / (w[1].tick - w[0].tick) as f32;
            w[0].entities.iter().zip(&w[1].entities).map(|(e0, e1)| {
                let p = [e0.1[0] + a * (e1.1[0] - e0.1[0]), e0.1[1] + a * (e1.1[1] - e0.1[1]), e0.1[2] + a * (e1.1[2] - e0.1[2])];
                (e0.0, p, e0.2) // rotation simplified
            }).collect()
        })
    }

    fn predict(&mut self, input: u8) { self.cmd_queue.push((self.seq, input)); }

    fn reconcile(&mut self, ack_id: u32, state: Snapshot) {
        self.cmd_queue.retain(|c| c.0 > ack_id);
        self.last_snap = Some(state);
    }
}


// CLIENT: let mut net = NetLayer::new("0.0.0.0:7001", false); loop { net.predict(input); net.send("127.0.0.1:7000", &net.cmd_queue, None); if let Some(p) = net.recv() { if let Some(s) = p.snap { net.reconcile(p.ack, s); }} if let Some(ents) = net.interpolate(net.tick.saturating_sub(net.interp_delay)) { render(ents); } net.tick += 1; sleep(16ms); }
// SERVER: let mut net = NetLayer::new("0.0.0.0:7000", true); let mut state = Snapshot{tick:0, entities:vec![]}; loop { if let Some(p) = net.recv() { for (id, cmd, _) in p.cmds { apply_cmd(&mut state, id, cmd); } state.tick += 1; net.send(peer, &[], Some(state.clone())); } sleep(15.625ms); }
