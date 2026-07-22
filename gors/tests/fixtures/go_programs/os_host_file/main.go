package main

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
)

func must(err error) {
	if err != nil {
		panic(err.Error())
	}
}

func baseName(name string) string {
	for i := len(name) - 1; i >= 0; i-- {
		if name[i] == '/' || name[i] == '\\' {
			return name[i+1:]
		}
	}
	return name
}

func isPathError(err error) bool {
	_, ok := err.(*os.PathError)
	return ok
}

func main() {
	file, err := os.CreateTemp("", "gors-host-file-*.bin")
	must(err)
	name := file.Name()
	defer os.Remove(name)

	stringWritten, err := file.WriteString("abcdef")
	must(err)
	readFrom, err := file.ReadFrom(bytes.NewBufferString("gh"))
	must(err)
	_, err = file.Seek(0, io.SeekStart)
	must(err)
	var copied bytes.Buffer
	writeTo, err := file.WriteTo(&copied)
	must(err)
	atWritten, err := file.WriteAt([]byte("XY"), 2)
	must(err)
	_, err = file.Seek(0, io.SeekStart)
	must(err)

	buffer := make([]byte, 8)
	read, err := file.Read(buffer)
	must(err)

	at := make([]byte, 3)
	atRead, err := file.ReadAt(at, 1)
	must(err)
	position, err := file.Seek(0, io.SeekCurrent)
	must(err)
	info, err := file.Stat()
	must(err)

	one := make([]byte, 1)
	eofRead, eof := file.Read(one)
	must(file.Close())

	opened, err := os.Open(name)
	must(err)
	reopened := make([]byte, 8)
	reopenedRead, err := opened.Read(reopened)
	must(err)
	must(opened.Close())

	exclusiveFile, exclusiveErr := os.OpenFile(
		name,
		os.O_WRONLY|os.O_CREATE|os.O_EXCL,
		0600,
	)

	readOnlyName := name + ".readonly"
	defer os.Remove(readOnlyName)
	readOnly, err := os.OpenFile(
		readOnlyName,
		os.O_RDONLY|os.O_CREATE|os.O_EXCL,
		0600,
	)
	must(err)
	readOnlySeed, err := os.OpenFile(readOnlyName, os.O_WRONLY, 0)
	must(err)
	_, err = readOnlySeed.WriteString("r")
	must(err)
	must(readOnlySeed.Close())
	readOnlyBuffer := make([]byte, 1)
	readOnlyRead, err := readOnly.Read(readOnlyBuffer)
	must(err)
	readOnlyWritten, readOnlyWriteErr := readOnly.Write([]byte("x"))
	readOnlyInfo, err := readOnly.Stat()
	must(err)
	must(readOnly.Close())

	truncated, err := os.OpenFile(name, os.O_WRONLY|os.O_TRUNC, 0)
	must(err)
	must(truncated.Close())
	truncated, err = os.Open(name)
	must(err)
	truncatedInfo, err := truncated.Stat()
	must(err)
	must(truncated.Close())

	fmt.Println(stringWritten, readFrom, writeTo, copied.String())
	fmt.Println(atWritten, read, string(buffer))
	fmt.Println(atRead, string(at), position, info.Size(), info.Name() == baseName(name))
	fmt.Println(eofRead, eof == io.EOF, reopenedRead, string(reopened))
	fmt.Println(
		exclusiveFile == nil,
		isPathError(exclusiveErr),
		os.IsExist(exclusiveErr),
		errors.Is(exclusiveErr, os.ErrExist),
	)
	fmt.Println(
		readOnlyRead,
		string(readOnlyBuffer),
		readOnlyWritten,
		readOnlyWriteErr != nil,
		readOnlyInfo.Size(),
	)
	fmt.Println(truncatedInfo.Size())
}
