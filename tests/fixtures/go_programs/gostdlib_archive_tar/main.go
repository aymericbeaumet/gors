package main

import (
	"archive/tar"
	"fmt"
)

func main() {
	fmt.Println(tar.TypeReg, tar.TypeDir, tar.TypeSymlink)
}
