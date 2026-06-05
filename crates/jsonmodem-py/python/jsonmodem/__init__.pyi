from typing import Any, ClassVar, Generic, Iterable, Iterator, Literal, Optional, Sequence, Tuple, TypedDict, TypeAlias, TypeVar, Union, overload

JSONInput: TypeAlias = Union[str, bytes, bytearray, memoryview]
JSONByteInput: TypeAlias = Union[bytes, memoryview]
JSONValue: TypeAlias = Union[None, bool, float, str, list["JSONValue"], dict[str, "JSONValue"]]
PathPatterns: TypeAlias = Union[str, Sequence[str]]
_ByteViews = TypeVar("_ByteViews", Literal[False], Literal[True])
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

class JsonModem(Generic[_ByteViews]):
    @overload
    def __init__(
        self: "JsonModem[Literal[False]]",
        options: Optional[ParserOptions] = ...,
        *,
        paths: Optional[PathPatterns] = ...,
        byte_views: Literal[False] = ...,
    ) -> None: ...
    @overload
    def __init__(
        self: "JsonModem[Literal[True]]",
        options: Optional[ParserOptions] = ...,
        *,
        paths: Optional[PathPatterns] = ...,
        byte_views: Literal[True],
    ) -> None: ...

    @property
    def is_finished(self) -> bool: ...

    @overload
    def feed(
        self: "JsonModem[Literal[False]]",
        chunk_or_chunks: Union[JSONInput, Iterable[JSONInput]],
    ) -> Iterator[Event]: ...
    @overload
    def feed(
        self: "JsonModem[Literal[True]]",
        chunk_or_chunks: Union[JSONByteInput, Iterable[JSONByteInput]],
    ) -> Iterator[ByteViewEvent]: ...
    @overload
    def finish(self: "JsonModem[Literal[False]]") -> Iterator[Event]: ...
    @overload
    def finish(self: "JsonModem[Literal[True]]") -> Iterator[ByteViewEvent]: ...

class JsonModemValueView:
    @property
    def kind(self) -> Literal["empty", "null", "bool", "number", "string", "array", "object"]: ...
    @property
    def path(self) -> Path: ...
    def snapshot(self) -> JSONValue: ...
    def __getitem__(self, key: Union[str, int]) -> "JsonModemValueView": ...
    def __len__(self) -> int: ...

ValueUpdate: TypeAlias = Tuple[int, JsonModemValueView, PathView, bool]

class JsonModemValues:
    def __init__(self, options: Optional[ParserOptions] = ...) -> None: ...

    @property
    def is_finished(self) -> bool: ...

    def feed(self, chunk_or_chunks: Union[JSONInput, Iterable[JSONInput]]) -> Iterator[ValueUpdate]: ...
    def finish(self) -> Iterator[ValueUpdate]: ...
    def view(self) -> JsonModemValueView: ...

class JsonModemSyntaxError(Exception): ...
class JsonModemStateError(Exception): ...

__version__: str
