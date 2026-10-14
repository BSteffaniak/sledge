"""Qualified-mode behavioral matrix validator; does not manufacture release approval."""
from pathlib import Path
import sys
from validate_candidate import validate

if __name__ == '__main__':
    validate(Path(sys.argv[1]).read_text(), qualified_test=True)
    print('PASS_QUALIFIED_MATRIX_VM_ONLY_NOT_RELEASE_APPROVAL')
