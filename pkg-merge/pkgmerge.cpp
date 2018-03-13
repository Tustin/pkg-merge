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
#include <assert.h>

namespace fs = std::experimental::filesystem;
using std::string;
using std::map;
using std::vector;

struct Package {
	int					part;
	fs::path			file;
	vector<Package>		parts;
	bool operator < (const Package& rhs) const {
		return part < rhs.part;
	}
};

const char PKG_MAGIC[4] = { 0x7F, 0x43, 0x4E, 0x54 };


void merge(map<string, Package> packages) {
	for (auto & root : packages) {
		auto pkg = root.second;

		// Before we start, we need to sort the lists properly
		std::sort(pkg.parts.begin(), pkg.parts.end());

		size_t pieces = pkg.parts.size();
		auto title_id = root.first.c_str();

		printf("[work] beginning to merge %d %s for package %s...\n", pieces, pieces == 1 ? "piece" : "pieces", title_id);

		string merged_file_name = root.first + "-merged.pkg";
		string full_merged_file = pkg.file.parent_path().string() + "\\" + merged_file_name;

		if (fs::exists(full_merged_file)) {
			fs::remove(full_merged_file);
		}

		printf("\t[work] copying root package file to new file...");
		auto merged_file = fs::path(full_merged_file);

		// Deal with root file first
		fs::copy_file(pkg.file, merged_file, fs::copy_options::update_existing);
		printf("done\n");

		// Using C API from here on because it just works and is fast
		FILE *merged = fopen(full_merged_file.c_str(), "a+");

		// Now all the pieces...
		for (auto & part : pkg.parts) {
			FILE *to_merge = fopen(part.file.string().c_str(), "rb");

			auto total_size = fs::file_size(part.file);
			assert(total_size != 0);
			char b[1024 * 512];
			uintmax_t copied = 0;

			int n;
			while ((n = fread(b, 1, sizeof(b), to_merge)) > 0)
			{
				fwrite(b, 1, n, merged);
				copied += n;
				auto percentage = ((double)copied / (double)total_size) * 100;
				printf("\r\t[work] merged %llu/%llu bytes (%.0lf%%) for part %d...", copied, total_size, percentage, part.part);
			}
			fclose(to_merge);

			printf("done\n");
		}
		fclose(merged);
	}
}

int main(int argc, char *argv[])
{
	if (argc != 2) {
		std::cout << "No pkg directory supplied\nUsage: pkg-merge.exe <directory>" << std::endl;
		return 1;
	}
	string dir = argv[1];
	if (!fs::is_directory(dir)) {
		printf("[error] argument '%s' is not a directory\n", dir.c_str());
		return 1;
	}
	map<string, Package> packages;
	for (auto & file : fs::directory_iterator(dir)) {
		string file_name = file.path().filename().string();

		if (file.path().extension() != ".pkg") {
			printf("[warn] '%s' is not a PKG file. skipping...\n", file_name);
			continue;
		}

		if (file_name.find("-merged") != string::npos) continue;

		size_t found_part_begin = file_name.find_last_of("_") + 1;
		size_t found_part_end = file_name.find_first_of(".");
		string part = file_name.substr(found_part_begin, found_part_end - found_part_begin);
		string title_id = file_name.substr(0, found_part_begin - 1);
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
			piece.file = file.path();
			piece.part = pkg_piece;
			pkg->parts.push_back(piece);
			printf("[success] found piece %d for PKG file %s\n", pkg_piece, title_id.c_str());
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
		package.file = file.path();
		packages.insert(std::pair<string, Package>(title_id, package));
		printf("[success] found root PKG file for %s\n", title_id.c_str());

	}
	merge(packages);
	printf("\n[success] completed\n");
	return 0;
}




