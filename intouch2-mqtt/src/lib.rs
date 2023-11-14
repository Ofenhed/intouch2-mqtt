pub mod spa;
pub mod port_forward;

pub trait WithBuffer {
    type Buffer: AsRef<[u8]>;

    fn make_buffer() -> Self::Buffer;
}
