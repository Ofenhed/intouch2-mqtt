pub mod port_forward;
pub mod spa;

pub trait WithBuffer {
  type Buffer: AsRef<[u8]>;

  fn make_buffer() -> Self::Buffer;
}
