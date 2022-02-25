package reader

import "github.com/axieinfinity/steit-go/pkg/codec"

type IReader interface {
	Remaining() int
	ReadUint32() uint32
	ReadUint8() byte
	Read(int) []byte
	EndOfStream() bool
	ReadKey() (uint32, codec.WireType)
	GetNested() IReader
	SkipField(int)
}
