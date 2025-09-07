#[derive(Debug)]
#[allow(clippy::exhaustive_enums)]
pub enum RoomDisposition {
    /// A room version that has a stable specification.
    Stable,
    /// A room version that is not yet fully specified.
    Unstable,
}

#[derive(Debug)]
pub enum EventFormatVersion {
    /// $id:server event id format
    V1,
    /// MSC1659-style $hash event id format: introduced for room v3
    V2,
    /// MSC1884-style $hash format: introduced for room v4
    V3,
}

#[derive(Debug)]
pub enum StateResolutionVersion {
    /// State resolution for rooms at version 1.
    V1,
    /// State resolution for room at version 2 or later.
    V2,
}
