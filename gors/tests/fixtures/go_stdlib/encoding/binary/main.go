package main

import (
	"encoding/binary"
	"fmt"
)

func main() {
	fmt.Println("== encoding/binary/byte-order ==")
	caseByteOrder()
	fmt.Println("== encoding/binary/varint ==")
	caseVarint()
}

func caseByteOrder() {
	buf := []byte{4, 3, 2, 1, 5, 6}
	fmt.Println(binary.LittleEndian.Uint32(buf[:4]))
	fmt.Println(binary.BigEndian.Uint16(buf[4:6]))
}

func caseVarint() {
	buf := make([]byte, binary.MaxVarintLen64)
	n := binary.PutUvarint(buf, 300)
	value, read := binary.Uvarint(buf[:n])
	fmt.Println(n, value, read)
}
