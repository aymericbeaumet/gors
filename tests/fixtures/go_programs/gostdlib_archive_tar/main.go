package main

import (
	"archive/tar"
	"fmt"
)

func main() {
	fmt.Println(tar.TypeReg, tar.TypeDir, tar.TypeSymlink)
}

func coverArchiveTarAPI() {
	var _ = tar.ErrFieldTooLong
	var _ = tar.ErrHeader
	var _ = tar.ErrInsecurePath
	var _ = tar.ErrWriteAfterClose
	var _ = tar.ErrWriteTooLong
	var _ = tar.FileInfoHeader
	var _ tar.FileInfoNames
	var _ tar.Format
	var _ = tar.Format.String
	var _ = tar.FormatGNU
	var _ = tar.FormatPAX
	var _ = tar.FormatUnknown
	var _ = tar.FormatUSTAR
	var _ tar.Header
	var _ = (*tar.Header).FileInfo
	var _ = tar.NewReader
	var _ = tar.NewWriter
	var _ tar.Reader
	var _ = (*tar.Reader).Next
	var _ = (*tar.Reader).Read
	var _ = tar.TypeBlock
	var _ = tar.TypeChar
	var _ = tar.TypeCont
	var _ = tar.TypeDir
	var _ = tar.TypeFifo
	var _ = tar.TypeGNULongLink
	var _ = tar.TypeGNULongName
	var _ = tar.TypeGNUSparse
	var _ = tar.TypeLink
	var _ = tar.TypeReg
	var _ = tar.TypeRegA
	var _ = tar.TypeSymlink
	var _ = tar.TypeXGlobalHeader
	var _ = tar.TypeXHeader
	var _ tar.Writer
	var _ = (*tar.Writer).AddFS
	var _ = (*tar.Writer).Close
	var _ = (*tar.Writer).Flush
	var _ = (*tar.Writer).Write
	var _ = (*tar.Writer).WriteHeader
}
