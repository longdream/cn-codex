-- Full field-type test table for SmartBrain entry form tools.
-- Target: MySQL (测试数据库 / testform)
-- Covers: int/bigint/decimal/double, varchar/text, boolean, date/time/datetime/timestamp,
--         json, enum, nullable + required, auto-increment PK.

CREATE TABLE IF NOT EXISTS sb_entry_form_type_test (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT COMMENT '主键',
  supplier_name VARCHAR(100) NOT NULL COMMENT '供应商',
  contract_no VARCHAR(64) NULL COMMENT '合同编号',
  amount DECIMAL(12,2) NOT NULL COMMENT '合同金额',
  quantity INT NULL DEFAULT 1 COMMENT '数量',
  unit_price DOUBLE NULL COMMENT '单价',
  is_paid TINYINT(1) NOT NULL DEFAULT 0 COMMENT '是否已付款',
  status ENUM('draft','signed','closed') NOT NULL DEFAULT 'draft' COMMENT '状态',
  signed_date DATE NULL COMMENT '签约日期',
  signed_time TIME NULL COMMENT '签约时间',
  signed_at DATETIME NULL COMMENT '签约时间戳',
  created_ts TIMESTAMP NULL DEFAULT CURRENT_TIMESTAMP COMMENT '创建时间',
  remark TEXT NULL COMMENT '备注',
  meta_json JSON NULL COMMENT '扩展信息',
  blob_note BLOB NULL COMMENT '二进制备注(可选)',
  PRIMARY KEY (id),
  KEY idx_supplier_name (supplier_name),
  KEY idx_contract_no (contract_no)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci COMMENT='入库表单字段类型测试表';

-- Optional sample (commented):
-- INSERT INTO sb_entry_form_type_test
--   (supplier_name, contract_no, amount, quantity, unit_price, is_paid, status, signed_date, remark, meta_json)
-- VALUES
--   ('华为', 'HT-2026-001', 12000.00, 1, 12000.00, 0, 'signed', CURDATE(), '今天和华为签了12000元采购合同', JSON_OBJECT('source','manual'));
