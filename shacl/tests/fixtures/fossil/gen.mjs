import { readFileSync } from 'node:fs';
import { mapping } from '@fossil-lang/corpus';
process.stdout.write(mapping(readFileSync('fossil.json', 'utf8')));
