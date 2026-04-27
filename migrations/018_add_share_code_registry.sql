-- file_shares와 public_share_grants 두 테이블 사이의 share_code 충돌을 막기 위한
-- 단일 예약 테이블. INSERT 시점에 PK 제약으로 race condition을 차단한다.

CREATE TABLE share_codes (
    code CHAR(6) NOT NULL PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

INSERT IGNORE INTO share_codes (code, created_at)
SELECT share_code, created_at FROM file_shares;

INSERT IGNORE INTO share_codes (code, created_at)
SELECT share_code, created_at FROM public_share_grants;
