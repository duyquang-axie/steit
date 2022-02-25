package reader

var _ IReader = (*ByteReader)(nil)

type ByteReader struct {
	bytes []byte
	offset int
}

func NewByteReader(bytes []byte) IReader {
	return &ByteReader{bytes: bytes, offset: 0}
}

func (b *ByteReader) Remaining() int {
	return len(b.bytes) - b.offset
}

func (b *ByteReader) ReadUint8() byte {
	if b.Remaining() <= 0 {
		panic("end of stream")
	}

	n := b.bytes[b.offset]
	b.offset = b.offset + 1

	return n
}

func (b *ByteReader) Read(count int) []byte {
	if b.Remaining() <= 0 {
		panic("end of stream")
	}

	var bytes []byte

	for i := 0; i < count; i++ {
		bytes[i] = b.bytes[b.offset]
		b.offset = b.offset + 1
	}

	return bytes
}

func (b *ByteReader) ReadKey() (uint32, interface{}) {
	panic("implement me")
}

func (b *ByteReader) ReadUint32() uint32 {
	panic("implement me")
}

func (b *ByteReader) EndOfStream() bool {
	panic("implement me")
}

func (b *ByteReader) GetNested() IReader {
	panic("implement me")
}

func (b *ByteReader) SkipField(i int) {
	panic("implement me")
}

