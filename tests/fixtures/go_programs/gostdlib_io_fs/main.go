package main

import (
	"fmt"
	"io/fs"
)

func main() {
	fmt.Println(fs.ValidPath("a/b.txt"), fs.ValidPath("../x"), fs.ValidPath("."))
}
