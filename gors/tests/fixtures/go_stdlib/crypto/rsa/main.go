package main

import (
	"crypto"
	"crypto/rsa"
	"fmt"
)

func main() {
	// gors:stdlib-cover crypto/rsa::PSSSaltLengthAuto crypto/rsa::PSSSaltLengthEqualsHash
	fmt.Println(rsa.PSSSaltLengthAuto, rsa.PSSSaltLengthEqualsHash)

	// gors:stdlib-cover crypto/rsa::ErrDecryption crypto/rsa::ErrMessageTooLong crypto/rsa::ErrVerification
	fmt.Println(rsa.ErrDecryption.Error())
	fmt.Println(rsa.ErrMessageTooLong.Error())
	fmt.Println(rsa.ErrVerification.Error())

	// gors:stdlib-cover crypto/rsa::OAEPOptions crypto/rsa::PKCS1v15DecryptOptions
	oaep := rsa.OAEPOptions{Hash: crypto.SHA256, MGFHash: crypto.SHA512, Label: []byte("label")}
	pkcs := rsa.PKCS1v15DecryptOptions{SessionKeyLen: 16}
	fmt.Println(oaep.Hash == crypto.SHA256, oaep.MGFHash == crypto.SHA512, string(oaep.Label), pkcs.SessionKeyLen)

	// gors:stdlib-cover crypto/rsa::PSSOptions crypto/rsa::PSSOptions.HashFunc
	pss := &rsa.PSSOptions{SaltLength: rsa.PSSSaltLengthEqualsHash, Hash: crypto.SHA256}
	fmt.Println(pss.HashFunc() == crypto.SHA256, pss.SaltLength)
}
