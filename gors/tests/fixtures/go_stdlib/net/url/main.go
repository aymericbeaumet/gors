package main

import (
	"fmt"
	"net/url"
)

func main() {
	fmt.Println("== net/url/escape ==")
	path := url.PathEscape("my/cool+blog&about,stuff")
	query := url.QueryEscape("my/cool+blog&about,stuff")
	fmt.Println(path)
	fmt.Println(query)

	unpath, pathErr := url.PathUnescape(path)
	unquery, queryErr := url.QueryUnescape(query)
	fmt.Println(unpath, pathErr == nil)
	fmt.Println(unquery, queryErr == nil)
}
