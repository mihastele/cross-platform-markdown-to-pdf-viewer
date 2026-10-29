//! zlib/DEFLATE: a full inflater (stored, fixed and dynamic blocks) for PNG
//! decoding, and a small LZ77 + fixed-Huffman compressor for PDF streams.

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

// ------------------------------------------------------------- inflate

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    nbits: u32,
}

impl Bits<'_> {
    fn need(&mut self, n: u32) -> Result<(), String> {
        while self.nbits < n {
            let b = *self.data.get(self.pos).ok_or("unexpected end of deflate data")?;
            self.pos += 1;
            self.bit |= (b as u32) << self.nbits;
            self.nbits += 8;
        }
        Ok(())
    }
    fn get(&mut self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.nbits -= n;
        Ok(v)
    }
}

struct Huff {
    count: [u16; 16],
    symbol: Vec<u16>,
}

fn build(lengths: &[u8]) -> Huff {
    let mut count = [0u16; 16];
    for &l in lengths {
        count[l as usize] += 1;
    }
    let mut offs = [0u16; 16];
    for i in 1..15 {
        offs[i + 1] = offs[i] + count[i];
    }
    let mut symbol = vec![0u16; lengths.len()];
    for (s, &l) in lengths.iter().enumerate() {
        if l != 0 {
            symbol[offs[l as usize] as usize] = s as u16;
            offs[l as usize] += 1;
        }
    }
    Huff { count, symbol }
}

fn decode(br: &mut Bits, h: &Huff) -> Result<u16, String> {
    let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
    for len in 1..16 {
        code |= br.get(1)? as i32;
        let count = h.count[len] as i32;
        if code - count < first {
            return h.symbol.get((index + (code - first)) as usize).copied().ok_or_else(|| "bad huffman code".into());
        }
        index += count;
        first += count;
        first <<= 1;
        code <<= 1;
    }
    Err("bad huffman code".into())
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Inflates a raw DEFLATE stream.
pub fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut br = Bits { data, pos: 0, bit: 0, nbits: 0 };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = br.get(1)?;
        match br.get(2)? {
            0 => {
                br.bit = 0;
                br.nbits = 0;
                let p = br.pos;
                if p + 4 > data.len() {
                    return Err("truncated stored block".into());
                }
                let len = u16::from_le_bytes([data[p], data[p + 1]]) as usize;
                br.pos = p + 4;
                let chunk = data.get(br.pos..br.pos + len).ok_or("truncated stored block")?;
                out.extend_from_slice(chunk);
                br.pos += len;
            }
            t @ (1 | 2) => {
                let (lit, dist) = if t == 1 {
                    let mut l = [0u8; 288];
                    for (i, v) in l.iter_mut().enumerate() {
                        *v = match i {
                            0..=143 => 8,
                            144..=255 => 9,
                            256..=279 => 7,
                            _ => 8,
                        };
                    }
                    (build(&l), build(&[5u8; 30]))
                } else {
                    let nlen = br.get(5)? as usize + 257;
                    let ndist = br.get(5)? as usize + 1;
                    let ncode = br.get(4)? as usize + 4;
                    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                    let mut cl = [0u8; 19];
                    for &o in ORDER.iter().take(ncode) {
                        cl[o] = br.get(3)? as u8;
                    }
                    let clh = build(&cl);
                    let mut lengths = vec![0u8; nlen + ndist];
                    let mut i = 0;
                    while i < nlen + ndist {
                        let sym = decode(&mut br, &clh)?;
                        let (val, rep) = match sym {
                            0..=15 => (sym as u8, 1),
                            16 => (*lengths.get(i.wrapping_sub(1)).ok_or("bad repeat")?, 3 + br.get(2)? as usize),
                            17 => (0, 3 + br.get(3)? as usize),
                            _ => (0, 11 + br.get(7)? as usize),
                        };
                        if i + rep > nlen + ndist {
                            return Err("bad code lengths".into());
                        }
                        for _ in 0..rep {
                            lengths[i] = val;
                            i += 1;
                        }
                    }
                    (build(&lengths[..nlen]), build(&lengths[nlen..]))
                };
                loop {
                    let sym = decode(&mut br, &lit)? as usize;
                    match sym {
                        0..=255 => out.push(sym as u8),
                        256 => break,
                        257..=285 => {
                            let i = sym - 257;
                            let len = LEN_BASE[i] as usize + br.get(LEN_EXTRA[i] as u32)? as usize;
                            let ds = decode(&mut br, &dist)? as usize;
                            if ds >= 30 {
                                return Err("bad distance code".into());
                            }
                            let d = DIST_BASE[ds] as usize + br.get(DIST_EXTRA[ds] as u32)? as usize;
                            if d > out.len() {
                                return Err("distance too far back".into());
                            }
                            let start = out.len() - d;
                            for k in 0..len {
                                out.push(out[start + k]);
                            }
                        }
                        _ => return Err("bad literal/length code".into()),
                    }
                }
            }
            _ => return Err("bad block type".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

/// Decompresses a zlib stream (2-byte header, deflate data, adler32).
pub fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 6 {
        return Err("zlib stream too short".into());
    }
    inflate(&data[2..])
}

// ------------------------------------------------------------ compress

struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn put(&mut self, v: u32, bits: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// Huffman codes are packed most-significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        let mut r = 0;
        for i in 0..len {
            r |= ((code >> i) & 1) << (len - 1 - i);
        }
        self.put(r, len);
    }
    fn lit(&mut self, v: u32) {
        match v {
            0..=143 => self.code(0x30 + v, 8),
            144..=255 => self.code(0x190 + v - 144, 9),
            256..=279 => self.code(v - 256, 7),
            _ => self.code(0xc0 + v - 280, 8),
        }
    }
}

/// Compresses to a zlib stream using LZ77 with hash chains and fixed Huffman codes.
pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32768;
    const HASH: usize = 1 << 15;
    const MAX_CHAIN: usize = 24;
    let mut bw = BitWriter { out: vec![0x78, 0x9c], acc: 0, n: 0 };
    bw.put(1, 1);
    bw.put(1, 2);
    let mut head = vec![usize::MAX; HASH];
    let mut prev = vec![usize::MAX; data.len()];
    let hash = |i: usize| -> usize {
        ((data[i] as usize) << 10 ^ (data[i + 1] as usize) << 5 ^ data[i + 2] as usize) & (HASH - 1)
    };
    let mut i = 0;
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i + 3 <= data.len() {
            let h = hash(i);
            let mut cand = head[h];
            let mut chain = 0;
            while cand != usize::MAX && i - cand <= WINDOW && chain < MAX_CHAIN {
                let max = (data.len() - i).min(258);
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                    if l == max {
                        break;
                    }
                }
                cand = prev[cand];
                chain += 1;
            }
        }
        let advance = if best_len >= 3 {
            let li = LEN_BASE.iter().rposition(|&b| b as usize <= best_len).unwrap();
            bw.lit(257 + li as u32);
            bw.put((best_len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
            let di = DIST_BASE.iter().rposition(|&b| b as usize <= best_dist).unwrap();
            bw.code(di as u32, 5);
            bw.put((best_dist - DIST_BASE[di] as usize) as u32, DIST_EXTRA[di] as u32);
            best_len
        } else {
            bw.lit(data[i] as u32);
            1
        };
        for k in i..i + advance {
            if k + 3 <= data.len() {
                let h = hash(k);
                prev[k] = head[h];
                head[h] = k;
            }
        }
        i += advance;
    }
    bw.lit(256);
    if bw.n > 0 {
        bw.out.push(bw.acc as u8);
    }
    bw.out.extend_from_slice(&adler32(data).to_be_bytes());
    bw.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut data = Vec::new();
        for i in 0..20000u32 {
            data.push((i * 7 % 251) as u8);
            data.extend_from_slice(b"hello hello hello ");
        }
        let z = zlib_compress(&data);
        assert!(z.len() < data.len() / 2);
        assert_eq!(zlib_decompress(&z).unwrap(), data);
        assert_eq!(zlib_decompress(&zlib_compress(b"")).unwrap(), b"");
    }
}
