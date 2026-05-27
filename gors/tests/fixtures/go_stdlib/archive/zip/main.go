package main

import (
	"archive/zip"
	"fmt"
)

func main() {
	fmt.Println(zip.Store, zip.Deflate)
}

func coverArchiveZipAPI() {
	var _ zip.Compressor
	var _ zip.Decompressor
	var _ = zip.Deflate
	var _ = zip.ErrAlgorithm
	var _ = zip.ErrChecksum
	var _ = zip.ErrFormat
	var _ = zip.ErrInsecurePath
	var _ zip.File
	var _ = (*zip.File).DataOffset
	var _ = (*zip.File).Open
	var _ = (*zip.File).OpenRaw
	var _ zip.FileHeader
	var _ = (*zip.FileHeader).FileInfo
	var _ = (*zip.FileHeader).Mode
	var _ = (*zip.FileHeader).ModTime
	var _ = (*zip.FileHeader).SetMode
	var _ = (*zip.FileHeader).SetModTime
	var _ = zip.FileInfoHeader
	var _ = zip.NewReader
	var _ = zip.NewWriter
	var _ = zip.OpenReader
	var _ zip.ReadCloser
	var _ = (*zip.ReadCloser).Close
	var _ zip.Reader
	var _ = (*zip.Reader).Open
	var _ = (*zip.Reader).RegisterDecompressor
	var _ = zip.RegisterCompressor
	var _ = zip.RegisterDecompressor
	var _ = zip.Store
	var _ zip.Writer
	var _ = (*zip.Writer).AddFS
	var _ = (*zip.Writer).Close
	var _ = (*zip.Writer).Copy
	var _ = (*zip.Writer).Create
	var _ = (*zip.Writer).CreateHeader
	var _ = (*zip.Writer).CreateRaw
	var _ = (*zip.Writer).Flush
	var _ = (*zip.Writer).RegisterCompressor
	var _ = (*zip.Writer).SetComment
	var _ = (*zip.Writer).SetOffset
}
