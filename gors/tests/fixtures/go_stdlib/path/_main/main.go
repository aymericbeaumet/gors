package main

import (
	"fmt"
	"path"
)

func main() {
	fmt.Println("== path/base ==")
	case_path_base()
	fmt.Println("== path/dir ==")
	case_path_dir()
	fmt.Println("== path/ext ==")
	case_path_ext()
	fmt.Println("== path/is_abs ==")
	case_path_is_abs()
	fmt.Println("== path/join ==")
	case_path_join()
	fmt.Println("== path/clean_match ==")
	case_path_clean_match()
	fmt.Println("== path/split ==")
	case_path_split()
}

func case_path_base() {
	fmt.Println(path.Base("/alpha/beta.txt"))
	fmt.Println(path.Base("/"))
	fmt.Println(path.Base(""))
}

func case_path_dir() {
	fmt.Println(path.Dir("/alpha/beta.txt"))
	fmt.Println(path.Dir("beta.txt"))
	fmt.Println(path.Dir("/"))
}

func case_path_ext() {
	fmt.Println(path.Ext("archive.tar.gz"))
	fmt.Println(path.Ext("/alpha/beta"))
	fmt.Println(path.Ext(".profile"))
}

func case_path_is_abs() {
	fmt.Println(path.IsAbs("/alpha"))
	fmt.Println(path.IsAbs("alpha/beta"))
}

func case_path_join() {
	fmt.Println(path.Join("alpha", "beta", "file.txt"))
	fmt.Println(path.Join("", "alpha", "..", "beta"))
}

func case_path_clean_match() {
	fmt.Println(path.Clean("/alpha/../beta//file.txt"))
}

func case_path_split() {
	dir, file := path.Split("/alpha/beta.txt")
	fmt.Println(dir, file)
	dir, file = path.Split("beta.txt")
	fmt.Println(dir, file)
}
