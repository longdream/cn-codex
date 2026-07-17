// Copyright 2025 The Chromium Authors. All rights reserved.
// Copyright (C) Microsoft Corp. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
export class NamedFunctionRange {
    name;
    start;
    end;
    constructor(name, start, end) {
        this.name = name;
        this.start = start;
        this.end = end;
        if (start.line < 0 || start.column < 0 || end.line < 0 || end.column < 0) {
            throw new Error(`Line and column positions should be positive but were not: startLine=${start.line}, startColumn=${start.column}, endLine=${end.line}, endColumn=${end.column}`);
        }
        if (start.line > end.line || (start.line === end.line && start.column > end.column)) {
            throw new Error(`End position should be greater than start position: startLine=${start.line}, startColumn=${start.column}, endLine=${end.line}, endColumn=${end.column}`);
        }
    }
}
//# sourceMappingURL=NamedFunctionRange.js.map