package main

import (
	"debug/elf"
	"fmt"
)

func main() {
	fmt.Println(elf.ELFCLASS64 == elf.ELFCLASS64, elf.ELFDATA2LSB == elf.ELFDATA2MSB)
}
