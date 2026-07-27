use heapless::vec::Vec;

const BUFFER_SIZE: usize = 20;

struct DataBuffer<'a, T> {
    source: &'a str,
    buffer: Vec<T, BUFFER_SIZE, u16>,
}

pub struct DataManager {
    data_buffers: Vec<DataBuffer>,
}
