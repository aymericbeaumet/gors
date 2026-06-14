package main

import (
	"archive/zip"
	"fmt"
	"io/fs"
)

func main() {
	fmt.Println("== archive/zip/constants ==")
	caseConstants()
	fmt.Println("== archive/zip/errors ==")
	caseErrors()
	fmt.Println("== archive/zip/file-header ==")
	caseFileHeader()
}

func caseConstants() {
	// gors:stdlib-cover archive/zip::Store archive/zip::Deflate
	fmt.Println(zip.Store, zip.Deflate)
}

func caseErrors() {
	// gors:stdlib-cover archive/zip::ErrAlgorithm archive/zip::ErrChecksum archive/zip::ErrFormat archive/zip::ErrInsecurePath
	fmt.Println(zip.ErrAlgorithm.Error())
	fmt.Println(zip.ErrChecksum.Error())
	fmt.Println(zip.ErrFormat.Error())
	fmt.Println(zip.ErrInsecurePath.Error())
}

func caseFileHeader() {
	// gors:stdlib-cover archive/zip::FileHeader archive/zip::FileHeader.SetMode
	h := zip.FileHeader{
		Name:               "dir/file.txt",
		Method:             zip.Store,
		UncompressedSize64: 7,
	}
	h.SetMode(fs.ModeDir | 0755)
	fmt.Println(h.Name, h.Method, h.ExternalAttrs != 0, h.UncompressedSize64)
}
