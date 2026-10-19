#[derive(PartialEq, Clone, Copy, Debug, Eq)]
pub struct Generation(pub u64);

impl Generation {
    // u64 won't overflow even at 1 static data refresh per second
    pub fn bump(self) -> Self {
        Generation(self.0 + 1)
    }
}
