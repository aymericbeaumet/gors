package main

import (
	"crypto"
	"crypto/ed25519"
	"fmt"
)

func main() {
	fmt.Println("== ed25519/constants ==")
	// gors:stdlib-cover crypto/ed25519::PrivateKeySize
	// gors:stdlib-cover crypto/ed25519::PublicKeySize
	// gors:stdlib-cover crypto/ed25519::SeedSize
	// gors:stdlib-cover crypto/ed25519::SignatureSize
	fmt.Println(ed25519.PublicKeySize, ed25519.PrivateKeySize, ed25519.SignatureSize, ed25519.SeedSize)

	fmt.Println("== ed25519/options ==")
	// gors:stdlib-cover crypto/ed25519::Options
	// gors:stdlib-cover crypto/ed25519::Options.HashFunc
	opts := &ed25519.Options{Hash: crypto.SHA512}
	fmt.Println(opts.HashFunc() == crypto.SHA512)
}
