// pkgmerge.cpp : Defines the entry point for the console application.
//

#include "stdafx.h"
#include <stdio.h>
#include <string>
#include <iostream>
#include <fstream>
#include <filesystem>
#include <map>
#include <list>

namespace fs = std::experimental::filesystem;
using std::string;
using std::map;
using std::list;

struct Package {
	int					part;
	string				file;
	list<Package>		parts;
};

const char PKG_MAGIC[4] = { 0x7F, 0x43, 0x4E, 0x54 };

int main(int argc, char *argv[])
{
	map<string, Package> packages;
	for (auto & file : fs::directory_iterator("E:\\Code\\Visual Studio\\Visual Studio 2017\\Projects\\pkg-merge\\Debug\\pkgs")) {
		string file_name = file.path().filename().string();

		if (file.path().extension() != ".pkg") {
			printf("[warn] '%s' is not a PKG file. skipping...\n", file_name);
			continue;
		}

		size_t found = file_name.find_last_of("_");
		string part = file_name.substr(found + 1, 1);
		string title_id = file_name.substr(0, found - 1);
		char* ptr = NULL;
		auto pkg_piece = strtol(part.c_str(), &ptr, 10);
		if (ptr == NULL) {
			printf("[warn] '%s' is not a valid piece (fails integer conversion). skipping...\n", part.c_str());
			continue;
		}

		//Check if package exists
		auto it = packages.find(title_id);
		if (it != packages.end()) {
			//Exists, so add this as a piece
			auto pkg = &it->second;
			auto piece = Package();
			piece.file = file_name;
			piece.part = pkg_piece;
			pkg->parts.push_back(piece);
			continue;
		}

		//Wasn't found, so let's try to see if it's a root PKG file.
		std::ifstream ifs(file, std::ios::binary);
		char magic[4];
		ifs.read(magic, sizeof(magic));
		ifs.close();

		if (memcmp(magic, PKG_MAGIC, sizeof(PKG_MAGIC) != 0)) {
			printf("[warn] assumed root PKG file '%s' doesn't match PKG magic (is %x, wants %x). skipping...\n", file_name.c_str(), magic, PKG_MAGIC);
			continue;
		}

		auto package = Package();
		package.part = 0;
		package.file = file_name;
		packages.insert(std::pair<string, Package>(title_id, package));
		printf("[success] found root PKG file for %s\n", title_id.c_str());

	}
	printf("%d\n", argc);
    return 0;
}

