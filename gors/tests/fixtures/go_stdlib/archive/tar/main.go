package main

import (
	"archive/tar"
	"fmt"
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
