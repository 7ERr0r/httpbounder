



use crate::marker::BcDataMarked;
use actix_web::web::Bytes;

fn trim_boundary(mut val: &[u8]) -> &[u8] {
    while val.len() > 0 {
        let v = val[0];
        if v == ' ' as u8 || v == '"' as u8 {
            val = &val[1..];
        } else {
            break;
        }
    }
    while val.len() > 0 {
        let last = val.len() - 1;
        let v = val[last];
        if v == ' ' as u8 || v == '"' as u8 {
            val = &val[..last];
        } else {
            break;
        }
    }

    val
}

fn make_boundary(val: &[u8]) -> Vec<u8> {
    let mut bound: Vec<u8> = Vec::new();
    if val.len() >= 2 && val[0] != '-' as u8 {
        bound.push('-' as u8);
        bound.push('-' as u8);
    }
    bound.extend_from_slice(val);
    bound.push('\r' as u8);
    bound.push('\n' as u8);
    bound
}

pub struct MJPEGStartMarker {
    boundary: Option<Vec<u8>>,
}
impl MJPEGStartMarker {
    pub fn new() -> Self {
        Self { boundary: None }
    }
    pub fn read_headers(&mut self, headers: &reqwest::header::HeaderMap) {
        for (header_name, header_value) in headers.iter() {
            if *header_name == "content-type" {
                const PREFIX: &[u8] = b"multipart/x-mixed-replace;";
                let val = (*header_value).as_bytes();
                if val.starts_with(PREFIX) {
                    let val = &val[PREFIX.len()..];
                    let eq_pos = val.iter().position(|x| *x == '=' as u8);
                    match eq_pos {
                        Some(pos) => {
                            let val = trim_boundary(&val[(pos + 1)..]);
                            println!("mjpeg bound: {}", String::from_utf8_lossy(&val));

                            let bound = make_boundary(val);
                            self.boundary = Some(bound);
                        }
                        None => {}
                    }
                }
            }
        }
    }
    pub fn mark_chunk(&mut self, chunk: &Bytes) -> [Option<BcDataMarked>; 3] {
        match &self.boundary {
            None => [
                Some(BcDataMarked::new_valid_start(chunk.clone())),
                None,
                None,
            ],
            Some(bound) => {
                // TODO what if boundary sits half-way through bytes chunks?

                const FAST_BOUND_SEARCH: bool = true;
                if FAST_BOUND_SEARCH {
                    if chunk.ends_with(&bound) {
                        let mut a = chunk.clone();
                        let b = a.split_off(chunk.len() - bound.len());

                        let a = BcDataMarked::new_invalid(a);
                        let b = BcDataMarked::new_valid_start(b);
                        return [
                            if a.bytes.len() > 0 { Some(a) } else { None },
                            if b.bytes.len() > 0 { Some(b) } else { None },
                            None,
                        ];
                    }
                } else {
                    match twoway::find_bytes(&chunk, &bound) {
                        None => {}
                        Some(pos) => {
                            let mut a = chunk.clone();
                            let mut b = a.split_off(pos);
                            let c = b.split_off(bound.len());

                            let a = BcDataMarked::new_invalid(a);
                            let b = BcDataMarked::new_valid_start(b);
                            let c = BcDataMarked::new_invalid(c);
                            return [
                                if a.bytes.len() > 0 { Some(a) } else { None },
                                if b.bytes.len() > 0 { Some(b) } else { None },
                                if c.bytes.len() > 0 { Some(c) } else { None },
                            ];
                        }
                    }
                }

                [Some(BcDataMarked::new_invalid(chunk.clone())), None, None]
            }
        }
    }
}