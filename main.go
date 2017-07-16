package main

import (
	"bufio"
	"fmt"
	"io"
	"io/ioutil"
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

type Package struct {
	part  int
	root  bool
	file  os.FileInfo
	parts []*Package
}

var Packages = make(map[string]*Package)

func main() {
	files, err := ioutil.ReadDir("pkgs")

	if err != nil {
		panic(err)
	}

	for _, file := range files {
		if filepath.Ext(file.Name()) != ".pkg" {
			fmt.Println("not a pkg " + file.Name())
			continue
		}

		pieces := strings.Split(file.Name(), "_")
		pkgName := pieces[0]
		pkgPiece := 0
		if s, err := strconv.Atoi(strings.Split(pieces[len(pieces)-1], ".")[0]); err == nil {
			pkgPiece = s
		} else {
			fmt.Printf("not a valid piece %s\n", err.Error())
			continue
		}
		pkg := &Package{part: pkgPiece, file: file, parts: []*Package{}}

		//Found the pkg already
		if root, ok := Packages[pkgName]; ok {
			fmt.Printf("found piece %d for pkg %s\n", pkgPiece, pkgName)
			pkg.root = false
			root.parts = append(root.parts, pkg)
			continue
		}

		fmt.Printf("found new package %s\n", pkgName)
		pkg.root = true
		Packages[pkgName] = pkg
	}

	//merge them
	for titleId, pkg := range Packages {
		if err := merge(pkg); err == nil {
			fmt.Printf("Successfully merged %s\n", titleId)
		} else {
			panic(err)
		}
	}
}

func merge(pkg *Package) error {
	if !pkg.root {
		return fmt.Errorf("package is not the root package (part %d)", pkg.part)
	}
	rootFile, err := os.OpenFile(fmt.Sprintf("%s/%s", "pkgs", pkg.file.Name()), os.O_RDWR|os.O_APPEND, 0755)

	if err != nil {
		return fmt.Errorf("unable to open root package: %s", err.Error())
	}

	defer rootFile.Close()

	rootBuffer := bufio.NewWriter(rootFile)

	tempBuffer := make([]byte, 1024*1024)

	for _, part := range pkg.parts {
		fmt.Printf("merging part %d/%d\r", part.part, len(pkg.parts))
		partFile, err := os.OpenFile(fmt.Sprintf("%s/%s", "pkgs", part.file.Name()), os.O_RDONLY, 0755)

		if err != nil {
			return fmt.Errorf("unable to open package part %d: %s", part.part, err.Error())
		}

		partBuffer := bufio.NewReader(partFile)
		totalBytesWritten := 0

		for {
			_, err = io.ReadFull(partBuffer, tempBuffer)
			if err != nil {
				if err == io.EOF {
					break
				}

				return fmt.Errorf("failed reading part %d for %s: %s", part.part, part.file.Name(), err.Error())
			}

			written, err := rootBuffer.Write(tempBuffer)
			totalBytesWritten += written

			if err != nil {
				return fmt.Errorf("failed writing part %d for %s: %s", part.part, part.file.Name(), err.Error())
			}

			fmt.Printf("wrote %d/%d bytes\r", totalBytesWritten, part.file.Size())

		}
	}
	return nil
}
