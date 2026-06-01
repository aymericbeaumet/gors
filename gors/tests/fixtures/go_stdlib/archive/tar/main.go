package main

import (
	"archive/tar"
	"fmt"
)

func main() {
	// gors:stdlib-cover archive/tar::TypeReg archive/tar::TypeDir archive/tar::TypeSymlink
	fmt.Println(tar.TypeReg, tar.TypeDir, tar.TypeSymlink)
}
