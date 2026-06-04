from typing import Any, ClassVar, Iterable, Iterator, Literal, Optional, Tuple, TypedDict, TypeAlias, Union

JSONInput: TypeAlias = Union[str, bytes, bytearray, memoryview]
EventKind: TypeAlias = Literal[
    "null",
    "bool",
    "number",
    "string",
    "array_begin",
    "array_end",
    "object_begin",
    "object_end",
]
PathComponent: TypeAlias = Union[Tuple[Literal["key"], str], Tuple[Literal["index"], int]]
Path: TypeAlias = Tuple[PathComponent, ...]

class StringPayload:
    @property
    def fragment(self) -> str: ...
    @property
    def is_initial(self) -> bool: ...
    @property
    def is_final(self) -> bool: ...
    def as_dict(self) -> dict[str, object]: ...
    def __getitem__(self, key: str) -> object: ...

Payload: TypeAlias = Union[None, bool, float, StringPayload]

class PathView:
    def __len__(self) -> int: ...
    def __getitem__(self, index: Union[int, slice]) -> Union[PathComponent, Path]: ...
    def as_tuple(self) -> Path: ...
    def endswith(self, value: Union[str, Path]) -> bool: ...

Event: TypeAlias = Tuple[EventKind, PathView, Payload]

class ByteViewStringPayload(TypedDict):
    fragment: Union[memoryview, str]
    is_initial: bool
    is_final: bool
    is_view: bool

ByteViewPayload: TypeAlias = Union[None, bool, float, ByteViewStringPayload]
ByteViewEvent: TypeAlias = Tuple[EventKind, Path, ByteViewPayload]

class DecodeMode:
    StrictUnicode: ClassVar["DecodeMode"]
    SurrogatePreserving: ClassVar["DecodeMode"]
    ReplaceInvalid: ClassVar["DecodeMode"]

    def __init__(self, name: Optional[str] = ...) -> None: ...

    @property
    def name(self) -> str: ...

    @property
    def value(self) -> int: ...

class ParserOptions:
    def __init__(
        self,
        allow_unicode_whitespace: bool = ...,
        allow_multiple: bool = ...,
        decode_mode: Optional[DecodeMode] = ...,
        allow_uppercase_u: bool = ...,
    ) -> None: ...

    @property
    def allow_unicode_whitespace(self) -> bool: ...

    @property
    def allow_multiple(self) -> bool: ...

    @property
    def allow_uppercase_u(self) -> bool: ...

    @property
    def decode_mode(self) -> DecodeMode: ...

    def as_dict(self) -> dict[str, Any]: ...

class JsonModem:
    def __init__(self, options: Optional[ParserOptions] = ...) -> None: ...

    @property
    def is_finished(self) -> bool: ...

    def feed(self, chunk_or_chunks: Union[JSONInput, Iterable[JSONInput]]) -> Iterator[Event]: ...
    def finish(self) -> Iterator[Event]: ...

class JsonModemByteViews:
    def __init__(self, options: Optional[ParserOptions] = ...) -> None: ...

    @property
    def is_finished(self) -> bool: ...

    def feed(self, chunk: Union[bytes, memoryview]) -> Iterator[ByteViewEvent]: ...
    def finish(self) -> Iterator[ByteViewEvent]: ...

class JsonModemPathFilter:
    def __init__(
        self,
        paths: Union[str, list[str], tuple[str, ...]],
        *,
        options: Optional[ParserOptions] = ...,
        byte_views: bool = ...,
    ) -> None: ...

    @property
    def is_finished(self) -> bool: ...

    def feed(self, chunk: JSONInput) -> Iterator[Union[Event, ByteViewEvent]]: ...
    def finish(self) -> Iterator[Union[Event, ByteViewEvent]]: ...

class JsonModemSyntaxError(Exception): ...
class JsonModemStateError(Exception): ...

def loads(data: JSONInput) -> Any: ...
def string_ranges(data: bytes) -> list[Optional[tuple[int, int]]]: ...
def string_range_table(data: bytes) -> bytes: ...

__version__: str
