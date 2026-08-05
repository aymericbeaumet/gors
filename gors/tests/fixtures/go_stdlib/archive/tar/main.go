package main

import (
	"archive/tar"
	"fmt"
	"io"
	"io/fs"
	"time"
)

func main() {
	fmt.Println("== archive/tar/errors ==")
	caseErrors()
	fmt.Println("== archive/tar/typeflags ==")
	caseTypeflags()
	fmt.Println("== archive/tar/formats ==")
	caseFormats()
	fmt.Println("== archive/tar/header ==")
	caseHeader()
	fmt.Println("== archive/tar/header-file-info ==")
	caseHeaderFileInfo()
	fmt.Println("== archive/tar/file-info-header ==")
	caseFileInfoHeader()
	fmt.Println("== archive/tar/roundtrip ==")
	caseRoundTrip()
	fmt.Println("== archive/tar/add-fs ==")
	caseAddFS()
}

func caseErrors() {
	// gors:stdlib-cover archive/tar::ErrHeader archive/tar::ErrWriteTooLong archive/tar::ErrFieldTooLong archive/tar::ErrWriteAfterClose archive/tar::ErrInsecurePath
	fmt.Println(tar.ErrHeader.Error())
	fmt.Println(tar.ErrWriteTooLong.Error())
	fmt.Println(tar.ErrFieldTooLong.Error())
	fmt.Println(tar.ErrWriteAfterClose.Error())
	fmt.Println(tar.ErrInsecurePath.Error())
}

func caseTypeflags() {
	// gors:stdlib-cover archive/tar::TypeReg archive/tar::TypeRegA archive/tar::TypeLink archive/tar::TypeSymlink archive/tar::TypeChar archive/tar::TypeBlock archive/tar::TypeDir archive/tar::TypeFifo archive/tar::TypeCont archive/tar::TypeXHeader archive/tar::TypeXGlobalHeader archive/tar::TypeGNUSparse archive/tar::TypeGNULongName archive/tar::TypeGNULongLink
	fmt.Println(
		tar.TypeReg,
		tar.TypeRegA,
		tar.TypeLink,
		tar.TypeSymlink,
		tar.TypeChar,
		tar.TypeBlock,
		tar.TypeDir,
		tar.TypeFifo,
		tar.TypeCont,
		tar.TypeXHeader,
		tar.TypeXGlobalHeader,
		tar.TypeGNUSparse,
		tar.TypeGNULongName,
		tar.TypeGNULongLink,
	)
}

func caseFormats() {
	// gors:stdlib-cover archive/tar::Format archive/tar::Format.String archive/tar::FormatUnknown archive/tar::FormatUSTAR archive/tar::FormatPAX archive/tar::FormatGNU
	fmt.Println(tar.Format.String(tar.FormatUnknown))
	fmt.Println(tar.Format.String(tar.FormatUSTAR))
	fmt.Println(tar.Format.String(tar.FormatPAX))
	fmt.Println(tar.Format.String(tar.FormatGNU))
	combo := tar.FormatUSTAR | tar.FormatPAX
	fmt.Println(tar.Format.String(combo))
}

func caseHeader() {
	// gors:stdlib-cover archive/tar::Header
	h := tar.Header{
		Name:     "dir/file.txt",
		Mode:     0644,
		Uid:      1000,
		Gid:      1001,
		Size:     7,
		Typeflag: tar.TypeReg,
		Linkname: "target",
		Uname:    "owner",
		Gname:    "group",
		Format:   tar.FormatPAX,
		PAXRecords: map[string]string{
			"comment": "fixture",
		},
	}
	fmt.Println(h.Name, h.Mode, h.Uid, h.Gid, h.Size, h.Typeflag, h.Linkname, h.Uname, h.Gname)
	fmt.Println(tar.Format.String(h.Format), h.PAXRecords["comment"])
}

func caseHeaderFileInfo() {
	// gors:stdlib-cover archive/tar::Header.FileInfo
	file := tar.Header{
		Name:     "dir/file.txt",
		Mode:     0644,
		Size:     7,
		Typeflag: tar.TypeReg,
	}
	fileInfo := file.FileInfo()
	fmt.Println(fileInfo.Name(), fileInfo.Size(), fileInfo.IsDir(), int64(fileInfo.Mode()))

	dir := tar.Header{
		Name:     "top/sub/",
		Mode:     0755,
		Typeflag: tar.TypeDir,
	}
	dirInfo := dir.FileInfo()
	fmt.Println(dirInfo.Name(), dirInfo.Size(), dirInfo.IsDir(), int64(dirInfo.Mode()))
}

type namedInfo struct {
	name string
	size int64
	mode fs.FileMode
}

func (n namedInfo) Name() string {
	return n.name
}

func (n namedInfo) Size() int64 {
	return n.size
}

func (n namedInfo) Mode() fs.FileMode {
	return n.mode
}

func (n namedInfo) ModTime() time.Time {
	return time.Time{}
}

func (n namedInfo) IsDir() bool {
	return n.mode.IsDir()
}

func (n namedInfo) Sys() any {
	return nil
}

func (n namedInfo) Uname() (string, error) {
	return "owner", nil
}

func (n namedInfo) Gname() (string, error) {
	return "group", nil
}

func caseFileInfoHeader() {
	// gors:stdlib-cover archive/tar::FileInfoHeader archive/tar::FileInfoNames
	file, fileErr := tar.FileInfoHeader(namedInfo{name: "file.txt", size: 7, mode: 0644}, "")
	fmt.Println(fileErr == nil, file.Name, file.Size, file.Mode, file.Typeflag, file.Uname, file.Gname, file.Linkname)

	dir, dirErr := tar.FileInfoHeader(namedInfo{name: "subdir", mode: fs.ModeDir | 0755}, "")
	fmt.Println(dirErr == nil, dir.Name, dir.Size, dir.Mode, dir.Typeflag, dir.Uname, dir.Gname, dir.Linkname)

	link, linkErr := tar.FileInfoHeader(namedInfo{name: "shortcut", mode: fs.ModeSymlink | 0777}, "target.txt")
	fmt.Println(linkErr == nil, link.Name, link.Size, link.Mode, link.Typeflag, link.Uname, link.Gname, link.Linkname)
}

type captureWriter struct {
	data   []byte
	writes int
}

func (w *captureWriter) Write(p []byte) (int, error) {
	w.writes++
	w.data = append(w.data, p...)
	return len(p), nil
}

type sliceReader struct {
	data []byte
	pos  int
}

func (r *sliceReader) Read(p []byte) (int, error) {
	if r.pos >= len(r.data) {
		return 0, io.EOF
	}
	n := copy(p, r.data[r.pos:])
	r.pos += n
	return n, nil
}

type memoryFS struct{}

func (memoryFS) Open(name string) (fs.File, error) {
	switch name {
	case ".":
		return &memoryFile{info: namedInfo{name: ".", mode: fs.ModeDir | 0755}}, nil
	case "docs":
		return &memoryFile{info: namedInfo{name: "docs", mode: fs.ModeDir | 0755}}, nil
	case "docs/message.txt":
		data := []byte("hello from fs")
		return &memoryFile{
			info: namedInfo{name: "message.txt", size: int64(len(data)), mode: 0640},
			data: data,
		}, nil
	default:
		return nil, fs.ErrNotExist
	}
}

func (memoryFS) ReadDir(name string) ([]fs.DirEntry, error) {
	switch name {
	case ".":
		return []fs.DirEntry{
			fs.FileInfoToDirEntry(namedInfo{name: "docs", mode: fs.ModeDir | 0755}),
		}, nil
	case "docs":
		return []fs.DirEntry{
			fs.FileInfoToDirEntry(namedInfo{name: "message.txt", size: 13, mode: 0640}),
		}, nil
	default:
		return nil, fs.ErrNotExist
	}
}

type memoryFile struct {
	info namedInfo
	data []byte
	pos  int
}

func (f *memoryFile) Stat() (fs.FileInfo, error) {
	return f.info, nil
}

func (f *memoryFile) Read(p []byte) (int, error) {
	if f.pos >= len(f.data) {
		return 0, io.EOF
	}
	n := copy(p, f.data[f.pos:])
	f.pos += n
	return n, nil
}

func (f *memoryFile) Close() error {
	return nil
}

func caseRoundTrip() {
	// gors:stdlib-cover archive/tar::NewWriter archive/tar::Writer archive/tar::Writer.WriteHeader archive/tar::Writer.Write archive/tar::Writer.Flush archive/tar::Writer.Close
	sink := captureWriter{}
	writer := tar.NewWriter(&sink)
	headerErr := writer.WriteHeader(&tar.Header{
		Name:     "hello.txt",
		Mode:     0644,
		Size:     5,
		Typeflag: tar.TypeReg,
	})
	n, writeErr := writer.Write([]byte("hello"))
	flushErr := writer.Flush()
	closeErr := writer.Close()
	fmt.Println(headerErr == nil, n, writeErr == nil, flushErr == nil, closeErr == nil)
	fmt.Println(len(sink.data), sink.writes > 0, sink.data[0], sink.data[257], sink.data[258], sink.data[259], sink.data[260], sink.data[261])

	// gors:stdlib-cover archive/tar::NewReader archive/tar::Reader archive/tar::Reader.Next archive/tar::Reader.Read
	source := sliceReader{data: sink.data}
	reader := tar.NewReader(&source)
	header, nextErr := reader.Next()
	fmt.Println(nextErr == nil, header.Name, header.Size, header.Mode, header.Typeflag)
	buf := make([]byte, 8)
	readN, readErr := reader.Read(buf)
	fmt.Println(readN, readErr == nil || readErr == io.EOF, string(buf[:readN]))
	nextHeader, nextErr := reader.Next()
	fmt.Println(nextHeader == nil, nextErr == io.EOF, source.pos == len(source.data))
}

func caseAddFS() {
	// gors:stdlib-cover archive/tar::Writer.AddFS
	sink := captureWriter{}
	writer := tar.NewWriter(&sink)
	addErr := writer.AddFS(memoryFS{})
	closeErr := writer.Close()
	fmt.Println(addErr == nil, closeErr == nil, len(sink.data) > 0)

	reader := tar.NewReader(&sliceReader{data: sink.data})
	dirHeader, dirErr := reader.Next()
	fmt.Println(dirErr == nil, dirHeader.Name, dirHeader.Size, dirHeader.Mode, dirHeader.Typeflag)

	fileHeader, fileErr := reader.Next()
	buf := make([]byte, 32)
	n, readErr := reader.Read(buf)
	fmt.Println(fileErr == nil, fileHeader.Name, fileHeader.Size, fileHeader.Mode, fileHeader.Typeflag)
	fmt.Println(n, readErr == nil || readErr == io.EOF, string(buf[:n]))

	endHeader, endErr := reader.Next()
	fmt.Println(endHeader == nil, endErr == io.EOF)
}
