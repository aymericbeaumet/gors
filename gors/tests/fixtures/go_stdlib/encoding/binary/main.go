package main

import (
	"encoding/binary"
	"fmt"
)

func main() {
	fmt.Println("== encoding/binary/varint ==")
	caseVarint()
}

func caseVarint() {
	buf := make([]byte, binary.MaxVarintLen64)
	n := binary.PutUvarint(buf, 300)
	value, read := binary.Uvarint(buf[:n])
	fmt.Println(n, value, read)
}
