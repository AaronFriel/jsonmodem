/// Lexical hint so factories can distinguish ints vs floats.
pub enum NumberLexeme<'a> {
    Integer(&'a str), // no '.' and no exponent
    Float(&'a str),   // has '.' or exponent
}
