# pkg-merge
Merges .PKG parts into one PKG file. Used for PS4 PKG files that come split into 4gb parts from the PlayStation Store.


## Compiling from source
Install Go on your system: https://golang.org/dl/

Setup your Go environment: https://skife.org/golang/2013/03/24/go_dev_env.html

Pull in this repo to your Go workspace

Navigate to folder using your terminal/command prompt

Run 'go run main.go' to execute, or run 'go build' to generate a binary to execute


## Using
Create a folder in your the exe directory called 'pkgs'

Put all of your pkg files inside this folder

Run the program; It'll merge all the parts into the main root package file (the one with _0 at the end of the name)
