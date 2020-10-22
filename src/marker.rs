
use actix_web::web::Bytes;

//type BcData = Arc<Vec<u8>>;

pub type BcData = Bytes;

pub struct BcDataMarked {
    pub bytes: BcData,
    pub valid_start: bool,
}
impl BcDataMarked {
    pub fn new_valid_start(b: BcData) -> Self {
        Self {
            bytes: b,
            valid_start: true,
        }
    }
    pub fn new_invalid(b: BcData) -> Self {
        Self {
            bytes: b,
            valid_start: false,
        }
    }
}