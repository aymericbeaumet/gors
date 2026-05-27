package main

import (
	"fmt"
	"html/template"
)

func main() {
	fmt.Println(template.OK == template.OK, template.ErrAmbigContext == template.ErrOutputContext)
}
