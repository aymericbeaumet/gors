package main

import (
	"fmt"
	"net/http"
)

func main() {
	fmt.Println(http.StatusOK, http.StatusCreated, http.StatusNotFound)
}
