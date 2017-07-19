package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"io/ioutil"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
)

type Package struct {
	part  int
	root  bool
	file  os.FileInfo
	parts []*Package
}

var packages = make(map[string]*Package)
var directory = flag.String("dir", "", "Directory to read PKG files from")

func SplitLast(str string, delimiter string) []string {
	result := make([]string, 2)
	delimMatch := -1
	c := delimiter[0]
	for i := 0; i < len(str); i++ {
		if str[i] == c {
			delimMatch = i
		}
	}
	if delimMatch == -1 {
		return []string{}
	}
	result[0] = str[:delimMatch]
	result[1] = str[delimMatch+1:]
	return result
}

func main() {
	flag.Parse()

	if *directory == "" {
		fmt.Fprintf(os.Stderr, "No directory supplied\nUsage:\npkg-merge.exe -dir \"myPkgDirectory\"\n")
		os.Exit(1)
	}

	files, err := ioutil.ReadDir(*directory)

	if err != nil {
		panic(err)
	}

	for _, file := range files {
		if filepath.Ext(file.Name()) != ".pkg" {
			fmt.Println("not a pkg " + file.Name())
			continue
		}

		pieces := SplitLast(file.Name(), "_")
		pkgName := pieces[0]
		pkgPiece := 0
		if s, err := strconv.Atoi(strings.Split(pieces[1], ".")[0]); err == nil {
			pkgPiece = s
		} else {
			fmt.Printf("not a valid piece %s\n", err.Error())
			continue
		}
		pkg := &Package{part: pkgPiece, file: file, parts: []*Package{}}

		//Found the pkg already
		if root, ok := packages[pkgName]; ok {
			fmt.Printf("found piece %d for pkg %s\n", pkgPiece, pkgName)
			pkg.root = false
			root.parts = append(root.parts, pkg)
			continue
		}
		//Make sure this is actually the root pkg
		pkgHandle, err := os.OpenFile(fmt.Sprintf("%s/%s", *directory, pkg.file.Name()), os.O_RDWR|os.O_APPEND, 0755)

		if err != nil {
			fmt.Printf("unable to open root package %s\n", err.Error())
			continue
		}

		pkgBuffer := bufio.NewReader(pkgHandle)
		header := make([]byte, 4)
		_, _ = pkgBuffer.Read(header)
		if !reflect.DeepEqual(header, []byte{0x7F, 0x43, 0x4E, 0x54}) {
			fmt.Printf("assumed root package file for %s doesn't match PKG magic (is %v)\n", pkgName, header)
			pkgHandle.Close()
			continue
		}

		fmt.Printf("found new package %s\n", pkgName)
		pkg.root = true
		packages[pkgName] = pkg
		pkgHandle.Close()

	}

	//merge them
	for titleId, pkg := range packages {
		if err := merge(pkg); err == nil {
			fmt.Printf("successfully merged %s\n", titleId)
		} else {
			panic(err)
		}
	}
}

func merge(pkg *Package) error {
	if !pkg.root {
		return fmt.Errorf("package is not the root package (part %d)", pkg.part)
	}
	rootFile, err := os.OpenFile(fmt.Sprintf("%s/%s", *directory, pkg.file.Name()), os.O_RDWR|os.O_APPEND, 0755)

	if err != nil {
		return fmt.Errorf("unable to open root package: %s", err.Error())
	}

	defer rootFile.Close()

	rootBuffer := bufio.NewWriter(rootFile)

	for _, part := range pkg.parts {
		fmt.Printf("merging part %d/%d\n", part.part, len(pkg.parts))
		partFile, err := os.OpenFile(fmt.Sprintf("%s/%s", *directory, part.file.Name()), os.O_RDONLY, 0755)

		if err != nil {
			return fmt.Errorf("unable to open package part %d: %s", part.part, err.Error())
		}

		partBuffer := bufio.NewReader(partFile)

		if _, err := io.Copy(rootBuffer, partBuffer); err != nil {
			return fmt.Errorf("failed copying package part %d: %s", part.part, err.Error())
		}
	}
	return nil
}
