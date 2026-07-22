package main

import (
	"archive/zip"
	"bytes"
	"fmt"
	"hash/crc32"
	"io"
	"io/fs"
	"os"
	"testing/fstest"
	"time"
)

const (
	packageMethod uint16 = 93
	localMethod   uint16 = 94
	unknownMethod uint16 = 222
)

type passthroughWriteCloser struct {
	writer io.Writer
}

func (w *passthroughWriteCloser) Write(p []byte) (int, error) {
	return w.writer.Write(p)
}

func (w *passthroughWriteCloser) Close() error {
	return nil
}

func passthroughCompressor(writer io.Writer) (io.WriteCloser, error) {
	return &passthroughWriteCloser{writer: writer}, nil
}

func passthroughDecompressor(reader io.Reader) io.ReadCloser {
	return io.NopCloser(reader)
}

func fixtureTime() time.Time {
	return time.Date(2024, time.January, 2, 3, 4, 6, 0, time.UTC)
}

func must(err error) {
	if err != nil {
		panic(err.Error())
	}
}

func sameError(got error, want error) bool {
	return got != nil && got.Error() == want.Error()
}

func writeString(writer io.Writer, value string) {
	n, err := writer.Write([]byte(value))
	must(err)
	if n != len(value) {
		panic("short write")
	}
}

func main() {
	fmt.Println("== archive/zip/constants-errors ==")
	caseConstantsAndErrors()
	fmt.Println("== archive/zip/file-header ==")
	caseFileHeader()
	fmt.Println("== archive/zip/codecs ==")
	caseCodecs()
	fmt.Println("== archive/zip/writer ==")
	archive := buildArchive()
	fmt.Println("== archive/zip/reader ==")
	reader := readArchive(archive)
	fmt.Println("== archive/zip/checksum ==")
	caseChecksum(archive)
	fmt.Println("== archive/zip/copy ==")
	caseCopy(reader)
	fmt.Println("== archive/zip/open-reader ==")
	caseOpenReader(archive)
}

func caseConstantsAndErrors() {
	// gors:stdlib-cover archive/zip::Store archive/zip::Deflate archive/zip::ErrAlgorithm archive/zip::ErrChecksum archive/zip::ErrFormat archive/zip::ErrInsecurePath
	fmt.Println(zip.Store, zip.Deflate)
	fmt.Println(zip.ErrAlgorithm.Error())
	fmt.Println(zip.ErrChecksum.Error())
	fmt.Println(zip.ErrFormat.Error())
	fmt.Println(zip.ErrInsecurePath.Error())

	bad := []byte("not a zip archive")
	_, err := zip.NewReader(bytes.NewReader(bad), int64(len(bad)))
	fmt.Println(sameError(err, zip.ErrFormat))
}

func caseFileHeader() {
	// gors:stdlib-cover archive/zip::FileHeader archive/zip::FileHeader.FileInfo archive/zip::FileHeader.ModTime archive/zip::FileHeader.Mode archive/zip::FileHeader.SetModTime archive/zip::FileHeader.SetMode archive/zip::FileInfoHeader
	header := zip.FileHeader{
		Name:               "docs/",
		UncompressedSize64: 0,
	}
	header.SetMode(fs.ModeDir | 0750)
	header.SetModTime(fixtureTime())

	info := header.FileInfo()
	copied, err := zip.FileInfoHeader(info)
	must(err)

	fmt.Println(header.Name, int64(header.Mode()), header.ModTime().Unix())
	fmt.Println(info.Name(), info.IsDir(), int64(info.Mode()), info.ModTime().Unix())
	fmt.Println(copied.Name, int64(copied.Mode()), copied.ModTime().Unix())
}

func caseCodecs() {
	// gors:stdlib-cover archive/zip::Compressor archive/zip::Decompressor archive/zip::RegisterCompressor archive/zip::RegisterDecompressor
	var compressor zip.Compressor = passthroughCompressor
	var decompressor zip.Decompressor = passthroughDecompressor
	zip.RegisterCompressor(packageMethod, compressor)
	zip.RegisterDecompressor(packageMethod, decompressor)
	fmt.Println(compressor != nil, decompressor != nil)
}

func addHeaderEntry(writer *zip.Writer, name string, method uint16, value string) {
	header := &zip.FileHeader{
		Name:     name,
		Method:   method,
		Modified: fixtureTime(),
	}
	header.SetMode(0644)
	entry, err := writer.CreateHeader(header)
	must(err)
	writeString(entry, value)
}

func rawHeader(name string, method uint16, value []byte) *zip.FileHeader {
	header := &zip.FileHeader{
		Name:               name,
		Method:             method,
		Modified:           fixtureTime(),
		CRC32:              crc32.ChecksumIEEE(value),
		CompressedSize64:   uint64(len(value)),
		UncompressedSize64: uint64(len(value)),
	}
	header.SetMode(0644)
	return header
}

func addRawEntry(writer *zip.Writer, name string, method uint16, value string) {
	data := []byte(value)
	entry, err := writer.CreateRaw(rawHeader(name, method, data))
	must(err)
	writeString(entry, value)
}

func buildArchive() []byte {
	// gors:stdlib-cover archive/zip::Writer archive/zip::NewWriter archive/zip::Writer.AddFS archive/zip::Writer.Close archive/zip::Writer.Create archive/zip::Writer.CreateHeader archive/zip::Writer.CreateRaw archive/zip::Writer.Flush archive/zip::Writer.RegisterCompressor archive/zip::Writer.SetComment archive/zip::Writer.SetOffset
	var output bytes.Buffer
	prefix := []byte("PFX!")
	_, err := output.Write(prefix)
	must(err)

	writer := zip.NewWriter(&output)
	writer.SetOffset(int64(len(prefix)))
	must(writer.SetComment("fixture-comment"))
	writer.RegisterCompressor(localMethod, zip.Compressor(passthroughCompressor))

	entry, err := writer.Create("deflate.txt")
	must(err)
	writeString(entry, "deflated")

	addHeaderEntry(writer, "stored.txt", zip.Store, "stored")
	addHeaderEntry(writer, "global.txt", packageMethod, "global")
	addHeaderEntry(writer, "local.txt", localMethod, "local")
	addRawEntry(writer, "raw.txt", zip.Store, "raw")
	addRawEntry(writer, "unknown.txt", unknownMethod, "unknown")

	fsys := fstest.MapFS{
		"docs/message.txt": &fstest.MapFile{
			Data:    []byte("from fs"),
			Mode:    0640,
			ModTime: fixtureTime(),
		},
	}
	must(writer.AddFS(fsys))
	flushErr := writer.Flush()
	closeErr := writer.Close()
	fmt.Println(flushErr == nil, closeErr == nil, output.Len() > len(prefix))

	return append([]byte(nil), output.Bytes()...)
}

func findFile(reader *zip.Reader, name string) *zip.File {
	for _, file := range reader.File {
		if file.Name == name {
			return file
		}
	}
	panic("missing zip member: " + name)
}

func readArchive(data []byte) *zip.Reader {
	// gors:stdlib-cover archive/zip::File archive/zip::File.DataOffset archive/zip::File.Open archive/zip::File.OpenRaw archive/zip::Reader archive/zip::NewReader archive/zip::Reader.Open archive/zip::Reader.RegisterDecompressor
	reader, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	must(err)
	reader.RegisterDecompressor(localMethod, zip.Decompressor(passthroughDecompressor))
	fmt.Println(reader.Comment, len(reader.File))

	names := []string{
		"deflate.txt",
		"stored.txt",
		"global.txt",
		"local.txt",
		"raw.txt",
		"docs/message.txt",
	}
	for _, name := range names {
		file := findFile(reader, name)
		offset, offsetErr := file.DataOffset()
		must(offsetErr)
		opened, openErr := file.Open()
		must(openErr)
		body, readErr := io.ReadAll(opened)
		must(readErr)
		must(opened.Close())
		fmt.Println(file.Name, file.Method, offset >= 0, string(body))
	}

	unsupported := findFile(reader, "unknown.txt")
	_, err = unsupported.Open()
	fmt.Println(sameError(err, zip.ErrAlgorithm))

	raw, err := findFile(reader, "raw.txt").OpenRaw()
	must(err)
	rawBody, err := io.ReadAll(raw)
	must(err)
	fmt.Println(string(rawBody))

	opened, err := reader.Open("deflate.txt")
	must(err)
	openedBody, err := io.ReadAll(opened)
	must(err)
	must(opened.Close())
	fmt.Println(string(openedBody))

	return reader
}

func caseChecksum(data []byte) {
	reader, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	must(err)
	offset, err := findFile(reader, "stored.txt").DataOffset()
	must(err)

	corrupted := append([]byte(nil), data...)
	corrupted[int(offset)] ^= 0xff

	broken, err := zip.NewReader(bytes.NewReader(corrupted), int64(len(corrupted)))
	must(err)
	opened, err := findFile(broken, "stored.txt").Open()
	must(err)
	_, readErr := io.ReadAll(opened)
	closeErr := opened.Close()
	fmt.Println(sameError(readErr, zip.ErrChecksum), closeErr == nil)
}

func caseCopy(reader *zip.Reader) {
	// gors:stdlib-cover archive/zip::Writer.Copy
	var output bytes.Buffer
	writer := zip.NewWriter(&output)
	must(writer.Copy(findFile(reader, "deflate.txt")))
	must(writer.Close())

	copied, err := zip.NewReader(bytes.NewReader(output.Bytes()), int64(output.Len()))
	must(err)
	opened, err := copied.File[0].Open()
	must(err)
	body, err := io.ReadAll(opened)
	must(err)
	must(opened.Close())
	fmt.Println(len(copied.File), copied.File[0].Name, string(body))
}

func caseOpenReader(data []byte) {
	// gors:stdlib-cover archive/zip::OpenReader archive/zip::ReadCloser archive/zip::ReadCloser.Close
	file, err := os.CreateTemp("", "gors-archive-zip-*.zip")
	must(err)
	name := file.Name()
	defer os.Remove(name)

	n, err := file.Write(data)
	must(err)
	if n != len(data) {
		panic("short temp-file write")
	}
	must(file.Close())

	reader, err := zip.OpenReader(name)
	must(err)
	fmt.Println(n == len(data), reader.Comment, len(reader.File))
	fmt.Println(reader.Close() == nil)
}
