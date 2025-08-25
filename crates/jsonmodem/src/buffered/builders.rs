use super::value_like;

/// What became "available" immediately after applying an event.
pub enum AppliedRef<'a, V: value_like::JsonValueLike> {
    Nothing,
    String(&'a V::Str),   // at final fragment
    Array(&'a V::Array),  // at ArrayEnd
    Object(&'a V::Object) // at ObjectEnd
}

/// Outcome per applied event. `root_completed` is true if the root
/// value just finished (useful for JsonModemValues in "full" mode).
pub struct ApplyOutcome<'a, V: JsonValueLike> {
    pub just: AppliedRef<'a, V>,
    pub root_completed: bool,
}

/// Assemblers consume core events and maintain an in-progress tree.
pub trait TreeAssembler<V: JsonValueLike> {
    fn apply<P>(&mut self, evt: &ParseEvent<V::Scalars, P>) -> ApplyOutcome<'_, V>;
    fn root(&self) -> Option<&V>;
    fn into_root(self) -> Option<V>;
}
